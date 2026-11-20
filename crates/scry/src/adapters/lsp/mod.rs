//! scry's language server: findings as diagnostics in the editor.
//!
//! The host (`sorcerer-lsp`) does the protocol; this module is what a save means to scry. It
//! serves one project, the one found from the root the editor sends at initialize. A file
//! belongs to it when the nearest project root above the file is that root; the server
//! ignores every other file, and every file the project has no language for.
//!
//! What the editor shows is what `scry` would print: `shown_in_file`, read afresh from the
//! store each time, for open documents only. Publishing runs on one thread of its own, in the
//! order it was asked for, so the main loop never waits on a parse or a grammar build and a
//! close always lands after the open before it.

mod diagnostics;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use lsp_server::{Connection, ResponseError};
use lsp_types::{Diagnostic, ServerCapabilities};
use serde_json::Value;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;
use sorcerer_lsp::serve::unknown_method;
use sorcerer_lsp::{Client, Product};

use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::scry_toml::ScryToml;
use crate::adapters::system_clock::SystemClock;
use crate::adapters::wiring::{LiveWiring, Wiring};
use crate::ports::clock::Clock;
use crate::usecases::show_findings::shown_in_file;
pub use diagnostics::{STALE_LINE, diagnostic};

/// How long the server waits after the last save before it reviews: a burst of saves, or an
/// editor's autosave, becomes one pass. A constant, not a setting.
pub const SETTLE: Duration = Duration::from_secs(2);

/// What the server is built from. The live server takes the real environment, the user's
/// grammar cache and trust list, the 2-second settle, the system clock and the live wiring
/// ([`ServerConfig::live`]); tests give a short settle, a fixed clock, vendored grammars and
/// doubles.
pub struct ServerConfig<W: Wiring> {
    pub env: Arc<dyn Env>,
    /// Opens the project at a root; called afresh at each save, so config edits need no
    /// restart.
    pub open: OpenProject,
    pub cache: GrammarCache,
    /// The user's trusted projects; `None` when there is no place to keep the list.
    pub trust: Option<TrustList>,
    pub settle: Duration,
    pub clock: Arc<dyn Clock>,
    /// Builds a pass's wiring, given its two cancels: the review calls' and everything
    /// else's.
    pub wire: WirePass<W>,
}

/// Opens the project at a root.
pub type OpenProject = Box<dyn Fn(&Path) -> anyhow::Result<Project> + Send + Sync>;

/// Builds one pass's wiring from the project and the pass's review cancel and other cancel.
pub type WirePass<W> = Box<dyn Fn(&Project, Cancel, Cancel) -> anyhow::Result<W> + Send + Sync>;

impl ServerConfig<LiveWiring> {
    /// The real server: `Project::open`, the user's grammar cache and trust list, the
    /// 2-second settle, the system clock and `LiveWiring::open_with`.
    pub fn live(env: Arc<dyn Env>) -> ServerConfig<LiveWiring> {
        let for_wire = Arc::clone(&env);
        ServerConfig {
            open: Box::new(|root| Ok(Project::open(root)?)),
            cache: GrammarCache::user(&*env),
            trust: TrustList::user(&*env),
            settle: SETTLE,
            clock: Arc::new(SystemClock),
            wire: Box::new(move |project, review, rest| {
                LiveWiring::open_with(project, &*for_wire, review, rest)
            }),
            env,
        }
    }
}

/// Serves scry over a connection until the editor says `exit`.
pub fn serve<W: Wiring + 'static>(
    connection: Connection,
    cfg: ServerConfig<W>,
) -> anyhow::Result<()> {
    sorcerer_lsp::serve(connection, move |client, root| {
        ScryServer::start(cfg, client, root)
    })
}

/// The product the host serves: a handle on the server's shared state.
struct ScryServer<W: Wiring> {
    server: Arc<Server<W>>,
}

/// What the main loop, the publisher and the pass worker share.
struct Server<W: Wiring> {
    cfg: ServerConfig<W>,
    client: Client,
    /// The directory the editor opened, as it sent it: the project is opened from here.
    root: PathBuf,
    publisher: Mutex<Sender<Publish>>,
    /// The last error logged, so the same one is not logged again and again.
    last_error: Mutex<Option<String>>,
}

/// What the publisher thread is asked to do, in order.
enum Publish {
    /// This file's diagnostics, if it is open.
    File(PathBuf),
    /// No diagnostics for this file (it closed).
    Clear(PathBuf),
}

impl<W: Wiring + 'static> ScryServer<W> {
    /// The server for the root the editor sent (the working directory when it sent none),
    /// with its publisher thread started.
    fn start(cfg: ServerConfig<W>, client: Client, root: Option<PathBuf>) -> ScryServer<W> {
        let (publisher, to_publish) = channel();
        let server = Arc::new(Server {
            cfg,
            client,
            root: root.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            publisher: Mutex::new(publisher),
            last_error: Mutex::new(None),
        });
        start_publisher(Arc::downgrade(&server), to_publish);
        ScryServer { server }
    }
}

impl<W: Wiring + 'static> Product for ScryServer<W> {
    fn capabilities() -> ServerCapabilities {
        ServerCapabilities::default()
    }

    fn opened(&mut self, path: &Path) {
        self.server.publish(Publish::File(path.to_path_buf()));
    }

    fn closed(&mut self, path: &Path) {
        self.server.publish(Publish::Clear(path.to_path_buf()));
    }

    fn saved(&mut self, path: &Path) {
        self.server.publish(Publish::File(path.to_path_buf()));
    }

    fn request(&mut self, method: &str, _params: Value) -> Result<Value, ResponseError> {
        Err(unknown_method(method))
    }

    fn shutdown(&mut self) {}
}

/// The thread that publishes, one request at a time, for as long as the server lives.
fn start_publisher<W: Wiring + 'static>(server: Weak<Server<W>>, to_publish: Receiver<Publish>) {
    std::thread::spawn(move || {
        for what in to_publish {
            let Some(server) = server.upgrade() else {
                return;
            };
            server.publish_now(what);
        }
    });
}

impl<W: Wiring> Server<W> {
    /// Asks the publisher thread to publish.
    fn publish(&self, what: Publish) {
        let _ = lock(&self.publisher).send(what);
    }

    fn publish_now(&self, what: Publish) {
        match what {
            Publish::Clear(path) => self.client.publish(&path, Vec::new()),
            Publish::File(path) => self.publish_if_open(&path),
        }
    }

    /// The file's diagnostics, when the editor has it open and it is one of the project's
    /// files. A failure (the project's config, the store) is logged and publishes nothing; the
    /// next save tries again.
    fn publish_if_open(&self, path: &Path) {
        if !self.client.is_open(path) {
            return;
        }
        match self.diagnostics_for(path) {
            Ok(Some(diagnostics)) => {
                self.client.publish(path, diagnostics);
                *lock(&self.last_error) = None;
            }
            Ok(None) => {}
            Err(e) => self.log_error(&format!("{e:#}")),
        }
    }

    /// What scry shows in the file now, as diagnostics; `None` when the file is not the
    /// project's or the project has no language for it.
    fn diagnostics_for(&self, path: &Path) -> anyhow::Result<Option<Vec<Diagnostic>>> {
        let project = (self.cfg.open)(&self.root)?;
        let Some(file) = project_file(&project, path) else {
            return Ok(None);
        };
        let store = JsonFindingStore::at(project.state.join("scry/findings.json"));
        let settings = ScryToml::for_project(&project);
        let root = project.root.clone();
        let view = FilesView::new(Arc::new(project), self.cfg.cache.clone());
        let shown = shown_in_file(&store, &settings, &view, &file)?;
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let related = self.client.related_information();
        Ok(Some(
            shown
                .iter()
                .map(|s| diagnostic(s, &root, &text, related))
                .collect(),
        ))
    }

    /// Logs an error to the editor's log for the server, unless it is the one logged last.
    fn log_error(&self, text: &str) {
        let mut last = lock(&self.last_error);
        if last.as_deref() != Some(text) {
            self.client.log(&format!("scry: {text}"));
            *last = Some(text.to_string());
        }
    }
}

/// The path relative to the project's root, when the file is the project's (the nearest
/// project root above it is this project's) and the project has a language for it.
///
/// The editor's path may differ from the canonical one (a symlinked directory, `/var` and
/// `/private/var`), so the comparison is made on the canonical path.
fn project_file(project: &Project, path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let directory = canonical.parent()?;
    if Project::root_above(directory)? != project.root {
        return None;
    }
    let relative = canonical.strip_prefix(&project.root).ok()?.to_path_buf();
    project.languages.for_path(&relative)?;
    Some(relative)
}

/// The lock, also after a thread panicked while holding it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
