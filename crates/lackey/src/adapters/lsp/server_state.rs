//! What the server's threads share: the editor, the project root, the last looks, the marks,
//! the pending questions, the tasks.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_lsp::{Client, Jobs, WorkDone};

use crate::adapters::task_runner::TaskRunner;
use crate::domain::mediums::MarkSeverity;

use super::asks::PendingAsk;
use super::server_config::ServerConfig;
use super::tasks::ShownTask;
use super::working::WorkingOn;

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
    /// The files (relative) each running task was sent the editor's unsaved text of, so its
    /// next `write_file` there may land.
    pub told_unsaved: Mutex<HashMap<String, HashSet<PathBuf>>>,
    /// Each running task's progress line.
    pub progress: Mutex<HashMap<String, WorkDone>>,
    /// What is being worked on now (a task, a question, a claim), by its key, drawn on its line.
    pub working: Mutex<HashMap<String, WorkingOn>>,
    /// The finished tasks still marked.
    pub shown: Mutex<Vec<ShownTask>>,
    /// The marks a task's agent put, by the editor's path.
    pub said: Mutex<HashMap<PathBuf, Vec<SaidMark>>>,
    pub asks: Mutex<Vec<PendingAsk>>,
    /// How many questions have been put on a line: each one's id is `<task>-<count>`, never
    /// reused, even when two subagents ask at once and an earlier question is gone.
    pub asks_put: AtomicU64,
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
            told_unsaved: Mutex::default(),
            progress: Mutex::default(),
            working: Mutex::default(),
            shown: Mutex::default(),
            said: Mutex::default(),
            asks: Mutex::default(),
            asks_put: AtomicU64::new(0),
        }
    }

    /// The project, opened afresh; `None` (and logged) when it does not open.
    /// The project's state directory, where an edit sent to the editor is noted as a tool's;
    /// `None` when the project does not open.
    pub(super) fn state_dir(&self) -> Option<PathBuf> {
        self.project()?.state_dir().ok()
    }

    pub fn project(&self) -> Option<Project> {
        match (self.cfg.open)(&self.root) {
            Ok(project) => Some(project),
            Err(e) => {
                self.client
                    .log(&format!("lackey: the project does not open: {e:#}"));
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

    /// Whether a file's text may be read for a model (the agent, Jev, the researcher): inside
    /// the project, not sorcerer's or git's own, and not fenced. The fences are opened afresh,
    /// so a `not_evidence` entry added since holds at once; when the project does not open,
    /// nothing may be.
    ///
    /// Example: `.env`, `docs/plans/launch.md` under `not_evidence = ["docs/plans"]` and a
    /// task's journal open in a buffer are never fed to the agent, typed into or not.
    pub fn may_send(&self, path: &Path) -> bool {
        self.project()
            .and_then(|project| Fences::for_project(&project).ok())
            .is_some_and(|fences| self.may_send_under(path, &fences))
    }

    /// [`Server::may_send`] under fences already opened.
    pub fn may_send_under(&self, path: &Path, fences: &Fences) -> bool {
        self.relative(path).is_some() && fences.allows(path)
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
    "lackey: this project is not trusted; run \"lackey trust\" in it";

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
