//! What the server's threads share: the editor, the project root, the last looks, the marks,
//! the pending questions, the tasks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_lsp::{Client, Jobs, WorkDone};

use crate::adapters::task_runner::TaskRunner;
use crate::domain::mediums::MarkSeverity;

use super::asks::PendingAsk;
use super::server_config::ServerConfig;
use super::tasks::ShownTask;

/// The server, shared by the main loop, the idle jobs, the task's thread and its helpers'.
pub(super) struct Server {
    pub cfg: ServerConfig,
    pub client: Client,
    pub root: PathBuf,
    pub jobs: Jobs,
    /// Each open file's text at the last look, by the editor's path: the person's edits are
    /// what changed since. The server's own edits move it too, so they are never triaged.
    pub looks: Mutex<HashMap<PathBuf, String>>,
    /// `.git/HEAD` at the last look: a checkout or a pull moves it, and then every look starts
    /// over, saying nothing.
    pub head: Mutex<Option<String>>,
    /// The task that runs, with its runner (one at a time).
    pub runner: Mutex<Option<Arc<TaskRunner>>>,
    /// Each running task's look at each file (relative): what it last saw or wrote.
    pub task_looks: Mutex<HashMap<String, HashMap<PathBuf, String>>>,
    /// What each task last wrote to each file (relative), for its undo.
    pub written: Mutex<HashMap<String, HashMap<PathBuf, String>>>,
    /// Each running task's progress line.
    pub progress: Mutex<HashMap<String, WorkDone>>,
    /// The finished tasks still marked.
    pub shown: Mutex<Vec<ShownTask>>,
    /// The marks a task's agent put, by the editor's path.
    pub said: Mutex<HashMap<PathBuf, Vec<SaidMark>>>,
    pub asks: Mutex<Vec<PendingAsk>>,
}

/// A mark the agent put with `mark`.
#[derive(Debug, Clone)]
pub(super) struct SaidMark {
    pub task: String,
    pub line: u32,
    pub text: String,
    pub severity: MarkSeverity,
}

impl Server {
    pub fn new(cfg: ServerConfig, client: Client, root: PathBuf) -> Server {
        Server {
            cfg,
            client,
            root,
            jobs: Jobs::new(),
            looks: Mutex::default(),
            head: Mutex::default(),
            runner: Mutex::default(),
            task_looks: Mutex::default(),
            written: Mutex::default(),
            progress: Mutex::default(),
            shown: Mutex::default(),
            said: Mutex::default(),
            asks: Mutex::default(),
        }
    }

    /// The project, opened afresh; `None` (and logged) when it does not open.
    pub fn project(&self) -> Option<Project> {
        match (self.cfg.open)(&self.root) {
            Ok(project) => Some(project),
            Err(e) => {
                self.client
                    .log(&format!("apprentice: the project does not open: {e:#}"));
                None
            }
        }
    }

    /// The project when it is trusted; an untrusted one is said once and gives none.
    pub fn trusted_project(&self) -> Option<Project> {
        let project = self.project()?;
        let trusted = needs_trust(&project).is_empty()
            || self
                .cfg
                .trust
                .as_ref()
                .is_some_and(|list| list.is_trusted(&project.root));
        if !trusted {
            self.client
                .say_once("untrusted", lsp_types::MessageType::WARNING, NOT_TRUSTED);
            return None;
        }
        Some(project)
    }

    /// The editor's path as the project names it: relative to the root, with `/`; `None` for a
    /// file outside the project or under `.sorcerer/` (the notes are the agent's, not regions).
    pub fn relative(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let text = forward_slashes(relative);
        (!text.starts_with(".sorcerer/") && !text.starts_with(".git/")).then_some(text)
    }

    /// The editor's path of a file relative to the root.
    pub fn editor_path(&self, relative: &Path) -> PathBuf {
        self.root.join(relative)
    }

    /// A file's text as the editor has it, else as the disk has it, else empty.
    pub fn text_now(&self, path: &Path) -> String {
        self.client
            .text_of(path)
            .map(|(_, text)| text)
            .or_else(|| std::fs::read_to_string(path).ok())
            .unwrap_or_default()
    }
}

/// What an untrusted project is told, once.
pub(super) const NOT_TRUSTED: &str =
    "apprentice: this project is not trusted; run \"apprentice trust\" in it";

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
