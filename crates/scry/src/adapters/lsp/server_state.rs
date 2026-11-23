//! What the main loop, the publisher and the pass worker share.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::Sender;

use sorcerer_lsp::{Client, Jobs};

use super::passes;
use super::server_config::ServerConfig;
use super::trust;
use crate::adapters::wiring::Wiring;
use crate::usecases::show_findings::Shown;

/// What the main loop, the publisher and the pass worker share.
pub(super) struct Server<W: Wiring> {
    pub(super) cfg: ServerConfig<W>,
    pub(super) client: Client,
    /// The directory the editor opened, as it sent it: the project is opened from here.
    pub(super) root: PathBuf,
    pub(super) publisher: Mutex<Sender<Publish>>,
    /// The errors logged since the server last published, so each is logged once.
    pub(super) errors_logged: Mutex<BTreeSet<String>>,
    /// What the server last published for each open file, for the code actions on a line.
    /// Keyed by the path as the editor named it (`publish` and `code_actions` both use it),
    /// not the canonical path `project_file` compares: look it up with the editor's path.
    pub(super) published: Mutex<HashMap<PathBuf, Vec<Shown>>>,
    /// The settle wait before a pass, and the overtaking check.
    pub(super) jobs: Jobs,
    /// The files saved and not yet reviewed, and the pass running, if one is.
    pub(super) passes: Mutex<passes::Passes>,
    /// Whether this session asked to trust the project, and what waits for the answer.
    pub(super) trust_question: Mutex<trust::TrustQuestion>,
}

/// What the publisher thread is asked to do, in order.
pub(super) enum Publish {
    /// This file's diagnostics, if it is open.
    File(PathBuf),
    /// No diagnostics for this file (it closed).
    Clear(PathBuf),
    /// Every open file's diagnostics.
    AllOpen,
    /// Something to do once what was asked before it is published: a pass's progress line
    /// ends after the pass's findings show.
    After(Box<dyn FnOnce() + Send>),
}

/// The lock, also after a thread panicked while holding it.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
