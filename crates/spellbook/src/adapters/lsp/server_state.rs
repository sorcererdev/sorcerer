//! What the main loop shares with the commands.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_lsp::Client;

use super::plans::PlanCache;
use super::server_config::ServerConfig;

/// What the server shares.
pub(super) struct Server {
    pub(super) cfg: ServerConfig,
    pub(super) client: Client,
    /// The directory the editor opened, as it sent it: the project is opened from here, and
    /// the paths the editor is told are joined to it.
    pub(super) root: PathBuf,
    /// The files that have diagnostics published, by the path the editor names them with, so a
    /// closed file is cleared.
    pub(super) published: Mutex<HashMap<PathBuf, usize>>,
    /// The plans made and not applied yet.
    pub(super) plans: Mutex<PlanCache>,
}

impl Server {
    /// Replaces a file's marks in the editor (the file is relative to the root, with `/`), and
    /// remembers it so closing the file clears them.
    pub(super) fn publish(&self, file: &str, marks: Vec<lsp_types::Diagnostic>) {
        let path = self.root.join(file);
        lock(&self.published).insert(path.clone(), marks.len());
        self.client.publish(&path, marks);
    }

    /// Whether spellbook may run in the project: it needs no trust, or the user trusted it.
    pub(super) fn trusted(&self, project: &Project) -> bool {
        needs_trust(project).is_empty()
            || self
                .cfg
                .trust
                .as_ref()
                .is_some_and(|list| list.is_trusted(&project.root))
    }
}

/// What an untrusted project is told when a command is run.
pub(super) const NOT_TRUSTED: &str =
    "spellbook: this project is not trusted; run \"spellbook trust\" in it";

/// The lock, also after a thread panicked while holding it.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
