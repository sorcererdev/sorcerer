//! What the main loop shares with the commands.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use lsp_types::MessageType;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_lsp::Client;

use crate::adapters::wiring::wired;
use crate::domain::config::spellbook_config;
use crate::domain::finding::Finding;

use super::diagnostics::{FileDiagnostics, file_diagnostics};
use super::plans::PlanCache;
use super::project_paths::project_relative;
use super::server_config::ServerConfig;

/// What the server shares.
pub(super) struct Server {
    pub(super) cfg: ServerConfig,
    pub(super) client: Client,
    /// The directory the editor opened, as it sent it: the project is opened from here, and
    /// the paths the editor is told are joined to it.
    pub(super) root: PathBuf,
    /// The files that have diagnostics published, by the path the editor names them with, and
    /// the findings among them: a closed file is cleared, and a code action reads a line's
    /// findings from here without checking again.
    pub(super) published: Mutex<HashMap<PathBuf, Vec<Finding>>>,
    /// The plans made and not applied yet.
    pub(super) plans: Mutex<PlanCache>,
}

impl Server {
    /// Replaces a file's marks and findings in the editor (the file is relative to the root,
    /// with `/`), and remembers them so closing the file clears them and a code action finds
    /// the findings.
    pub(super) fn publish(&self, file: &str, diagnostics: FileDiagnostics) {
        self.publish_at(&self.root.join(file), diagnostics);
    }

    /// The same, for a path as the editor names it.
    fn publish_at(&self, path: &Path, diagnostics: FileDiagnostics) {
        lock(&self.published).insert(path.to_path_buf(), diagnostics.findings.clone());
        self.client.publish(path, diagnostics.all());
    }

    /// The findings published for a file, by the path the editor names it with.
    pub(super) fn published_findings(&self, path: &Path) -> Vec<Finding> {
        lock(&self.published).get(path).cloned().unwrap_or_default()
    }

    /// Publishes the marks of a file the editor opened or saved, from the file on disk. Nothing
    /// for a file outside the project, or in a project that is not trusted.
    pub(super) fn republish(&self, path: &Path) {
        let Ok(project) = (self.cfg.open)(&self.root) else {
            return;
        };
        if !self.trusted(&project) {
            self.client
                .say_once("trust", MessageType::WARNING, NOT_TRUSTED);
            return;
        }
        if !spellbook_config(&project).is_ok_and(|config| config.enabled) {
            return;
        }
        let Some(file) = project_relative(&project, path) else {
            return;
        };
        let Ok(wired) = wired(project, self.cfg.cache.clone(), self.cfg.clock.clone()) else {
            return;
        };
        let diagnostics = file_diagnostics(&wired, &file);
        if let Some(problem) = &diagnostics.rules_problem {
            self.client.say_once(
                "rules",
                MessageType::WARNING,
                &format!("spellbook: {problem}"),
            );
        }
        self.publish_at(path, diagnostics);
    }

    /// Clears the marks of a file the editor closed.
    pub(super) fn closed(&self, path: &Path) {
        if lock(&self.published).remove(path).is_some() {
            self.client.publish(path, Vec::new());
        }
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
