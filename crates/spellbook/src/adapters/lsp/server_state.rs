//! What the main loop shares with the commands.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lsp_types::MessageType;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_lsp::Client;

use crate::adapters::wiring::{Wired, wired};
use crate::domain::config::spellbook_config;
use crate::domain::finding::Finding;
use crate::usecases::planned_view::PlannedView;

use super::diagnostics::{FileDiagnostics, file_diagnostics, file_diagnostics_over};
use super::plans::PlanCache;
use super::project_paths::project_relative;
use super::server_config::ServerConfig;

/// A file a fix lands in: its path in the plan (relative to the root, with `/`) and the text
/// the plan leaves in it.
#[derive(Debug, Clone)]
pub(super) struct Landing {
    pub(super) file: String,
    pub(super) text: String,
}

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
    /// The files a fix is landing in the editor, or has landed and the person has not saved
    /// yet, by the path the editor names them with: each with its path in the plan and the text
    /// the plan leaves in it. Until the file is saved (or closed, or the editor refuses the
    /// edit) its disk text is the old one, so its marks are read from this text: an editor that
    /// opens the file to land the edit must not get the disk's marks back.
    pub(super) landing: Mutex<HashMap<PathBuf, Landing>>,
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
        let diagnostics = self.diagnostics_now(&wired, path, &file);
        if let Some(problem) = &diagnostics.rules_problem {
            self.client.say_once(
                "rules",
                MessageType::WARNING,
                &format!("spellbook: {problem}"),
            );
        }
        self.publish_at(path, diagnostics);
    }

    /// A file's marks and findings: over the text a fix leaves in it while that fix lands (or
    /// is landed and not saved), else over the disk.
    fn diagnostics_now(&self, wired: &Wired, path: &Path, file: &Path) -> FileDiagnostics {
        let landing = lock(&self.landing);
        if !landing.contains_key(path) {
            drop(landing);
            return file_diagnostics(wired, file);
        }
        drop(landing);
        let planned = PlannedView::new(&wired.view, self.landing_texts());
        file_diagnostics_over(wired, &planned, file)
    }

    /// The text a fix or a sync left in each file the editor has not saved since, by its path
    /// in the plan.
    pub(super) fn landing_texts(&self) -> BTreeMap<String, String> {
        lock(&self.landing)
            .values()
            .map(|landed| (landed.file.clone(), landed.text.clone()))
            .collect()
    }

    /// The editor saved a file: (1) its disk text is the editor's now, so its marks come from
    /// the disk again; (2) the owned regions are synced (`sync_on_save.rs`); (3) the file's
    /// marks are published, then those of every other open file a region keeps, since the save
    /// may have made its block stale or fresh.
    pub(super) fn saved(self: &Arc<Self>, path: &Path) {
        self.forget_landing(path);
        self.sync_on_save();
        self.republish(path);
        self.republish_region_files(path);
    }

    /// Forgets the planned text of a file that was saved or closed.
    pub(super) fn forget_landing(&self, path: &Path) {
        lock(&self.landing).remove(path);
    }

    /// Clears the marks of a file the editor closed.
    pub(super) fn closed(&self, path: &Path) {
        self.forget_landing(path);
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
