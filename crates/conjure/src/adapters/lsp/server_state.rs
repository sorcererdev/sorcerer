//! What the main loop and the thread conjuring a change share.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use lsp_types::MessageType;
use sorcerer_core::Cancel;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_lsp::Client;

use super::changes::Shown;
use super::marks::Mark;
use super::server_config::ServerConfig;

/// What the main loop and the thread conjuring a change share.
pub(super) struct Server {
    pub(super) cfg: ServerConfig,
    pub(super) client: Client,
    /// The directory the editor opened, as it sent it: the project is opened from here.
    pub(super) root: PathBuf,
    /// The marks published for each file, by the path the editor names it with, for the actions
    /// on a line.
    pub(super) published: Mutex<HashMap<PathBuf, Vec<Mark>>>,
    /// The change marked now, if any.
    pub(super) shown: Mutex<Option<Shown>>,
    /// The cancel of the change being conjured, while one is.
    pub(super) running: Mutex<Option<Cancel>>,
}

impl Server {
    /// Says once a session that this editor cannot take conjure's edits, so nothing is
    /// offered.
    pub(super) fn say_once_if_it_cannot_edit(&self) {
        if !self.client.can_apply_edits() {
            self.client
                .say_once("edits", MessageType::WARNING, CANNOT_EDIT);
        }
    }

    /// Whether conjure may run in the project: it needs no trust, or the user trusted it.
    pub(super) fn trusted(&self, project: &Project) -> bool {
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
pub(super) const NOT_TRUSTED: &str =
    "conjure: this project is not trusted; run \"conjure trust\" in it";

/// The lock, also after a thread panicked while holding it.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
