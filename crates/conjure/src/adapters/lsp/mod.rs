//! conjure's language server: the code actions that conjure a change, the change landed in the
//! editor through one `workspace/applyEdit`, its marks, and what the person does with it.
//!
//! The host (`sorcerer-lsp`) does the protocol and keeps the open documents' text; this module
//! is what conjure does with them. It serves one project, the one found from the root the
//! editor sends at initialize, and ignores every file that is not the project's.
//!
//! - `commands.rs`: the capabilities, the code actions on a line and the commands they run.
//! - `changes.rs`: conjuring a change on a thread, landing it, and the change's life after:
//!   Undo, Try harder, Show why, Show <file>, and the choice a save, an undo or the next change
//!   records.
//! - `landing.rs`: a change as one `WorkspaceEdit`, and its undo.
//! - `marks.rs`: the marks (diagnostics) a landed change shows and their words.
//!
//! One change is marked at a time (`shown`), and one is conjured at a time (`running`).

pub(crate) mod changes;
pub(crate) mod commands;
mod landing;
mod marks;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lsp_server::{Connection, ResponseError};
use lsp_types::{MessageType, ServerCapabilities};
use serde_json::Value;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_lsp::serve::unknown_method;
use sorcerer_lsp::{Client, Product};

use crate::adapters::system_clock::SystemClock;
use crate::adapters::wiring::writer_for;
use crate::domain::config::ConjureConfig;
use crate::domain::record::Attempt;
use crate::ports::clock::Clock;
use crate::ports::writer::Writer;
use changes::Shown;
use marks::Mark;

/// What the server is built from. The live server takes the real environment, the user's
/// grammar cache and trust list, the system clock and the live writers ([`ServerConfig::live`]);
/// tests give vendored grammars, a fixed clock and a scripted writer.
pub struct ServerConfig {
    /// Opens the project at a root; called afresh for each change, so config edits need no
    /// restart.
    pub open: OpenProject,
    pub cache: GrammarCache,
    /// The user's trusted projects; `None` when there is no place to keep the list.
    pub trust: Option<TrustList>,
    pub clock: Arc<dyn Clock>,
    /// Builds the writer of an attempt, reading the project through the editor's view.
    pub wire: WireWriter,
}

/// Opens the project at a root.
pub type OpenProject = Box<dyn Fn(&Path) -> anyhow::Result<Project> + Send + Sync>;

/// Builds the writer for a project, its settings and an attempt; the view is the editor's, so a
/// writer that reads the project (Try harder) reads the unsaved text.
pub type WireWriter = Box<
    dyn Fn(
            &Project,
            &ConjureConfig,
            Attempt,
            Arc<dyn CodebaseView>,
        ) -> anyhow::Result<Box<dyn Writer>>
        + Send
        + Sync,
>;

impl ServerConfig {
    /// The real server: `Project::open`, the user's grammar cache and trust list, the system
    /// clock and `writer_for`.
    pub fn live(env: Arc<dyn Env>) -> ServerConfig {
        ServerConfig {
            open: Box::new(|root| Ok(Project::open(root)?)),
            cache: GrammarCache::user(&*env),
            trust: TrustList::user(&*env),
            clock: Arc::new(SystemClock),
            wire: Box::new(move |project, config, attempt, view| {
                writer_for(project, &*env, config, attempt, view)
            }),
        }
    }
}

/// Serves conjure over a connection until the editor says `exit`.
pub fn serve(connection: Connection, cfg: ServerConfig) -> anyhow::Result<()> {
    sorcerer_lsp::serve(connection, move |client, root| {
        ConjureServer::start(cfg, client, root)
    })
}

/// The product the host serves: a handle on the server's shared state.
struct ConjureServer {
    server: Arc<Server>,
}

/// What the main loop and the thread conjuring a change share.
struct Server {
    cfg: ServerConfig,
    client: Client,
    /// The directory the editor opened, as it sent it: the project is opened from here.
    root: PathBuf,
    /// The marks published for each file, by the path the editor names it with, for the actions
    /// on a line.
    published: Mutex<HashMap<PathBuf, Vec<Mark>>>,
    /// The change marked now, if any.
    shown: Mutex<Option<Shown>>,
    /// The cancel of the change being conjured, while one is.
    running: Mutex<Option<Cancel>>,
}

impl ConjureServer {
    /// The server for the root the editor sent (the working directory when it sent none).
    fn start(cfg: ServerConfig, client: Client, root: Option<PathBuf>) -> ConjureServer {
        let server = Arc::new(Server {
            cfg,
            client,
            root: root.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            published: Mutex::new(HashMap::new()),
            shown: Mutex::new(None),
            running: Mutex::new(None),
        });
        ConjureServer { server }
    }
}

impl Product for ConjureServer {
    fn capabilities() -> ServerCapabilities {
        commands::capabilities()
    }

    fn tracks_text() -> bool {
        true
    }

    fn opened(&mut self, path: &Path) {
        self.server.say_once_if_it_cannot_edit();
        self.server.republish(path);
    }

    fn changed(&mut self, path: &Path, _version: i32, _text: &str) {
        self.server.changed(path);
    }

    fn closed(&mut self, path: &Path) {
        // the marks follow the open set; they come back when the file opens again
        self.server.client.publish(path, Vec::new());
    }

    fn saved(&mut self, path: &Path) {
        self.server.saved(path);
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        match method {
            "textDocument/codeAction" => self.server.code_actions(params),
            "workspace/executeCommand" => self.server.execute(params),
            _ => Err(unknown_method(method)),
        }
    }

    fn shutdown(&mut self) {
        self.server.shutdown();
    }
}

impl Server {
    /// Says once a session that this editor cannot take conjure's edits, so nothing is
    /// offered.
    fn say_once_if_it_cannot_edit(&self) {
        if !self.client.can_apply_edits() {
            self.client
                .say_once("edits", MessageType::WARNING, CANNOT_EDIT);
        }
    }

    /// Whether conjure may run in the project: it needs no trust, or the user trusted it.
    fn trusted(&self, project: &Project) -> bool {
        needs_trust(project).is_empty()
            || self
                .cfg
                .trust
                .as_ref()
                .is_some_and(|list| list.is_trusted(&project.root))
    }
}

/// What an editor that cannot apply edits is told.
const CANNOT_EDIT: &str = "conjure: this editor cannot apply edits from a server (workspace/applyEdit with documentChanges)";

/// What an untrusted project is told when a change is asked for.
const NOT_TRUSTED: &str = "conjure: this project is not trusted; run \"conjure trust\" in it";

/// The path relative to the project's root, written with `/`, when the file is the project's
/// (the nearest project root above it is this project's), whatever its language.
///
/// The editor's path may differ from the canonical one (a symlinked directory, `/var` and
/// `/private/var`), so the comparison is made on the canonical path; a file not on disk yet
/// (a buffer the editor created) is resolved through its folder.
fn project_relative(project: &Project, path: &Path) -> Option<PathBuf> {
    let canonical = canonical(path).ok().or_else(|| {
        let folder = canonical(path.parent()?).ok()?;
        Some(folder.join(path.file_name()?))
    })?;
    let directory = canonical.parent()?;
    if Project::root_above(directory)? != project.root {
        return None;
    }
    Some(PathBuf::from(forward_slashes(
        canonical.strip_prefix(&project.root).ok()?,
    )))
}

/// The same, only for a file the project has a language for.
fn project_file(project: &Project, path: &Path) -> Option<PathBuf> {
    let relative = project_relative(project, path)?;
    project.languages.for_path(&relative)?;
    Some(relative)
}

/// The lock, also after a thread panicked while holding it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
