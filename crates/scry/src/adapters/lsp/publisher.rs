//! Publishing diagnostics: the publisher thread, and what the editor shows for a file.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Weak};

use lsp_types::Diagnostic;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;

use super::diagnostics::diagnostic;
use super::server_state::{Publish, Server, lock};
use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::scry_toml::ScryToml;
use crate::adapters::wiring::Wiring;
use crate::usecases::show_findings::{Shown, shown_in_file};

/// The thread that publishes, one request at a time, for as long as the server lives.
pub(super) fn start_publisher<W: Wiring + 'static>(
    server: Weak<Server<W>>,
    to_publish: Receiver<Publish>,
) {
    std::thread::spawn(move || {
        for what in to_publish {
            let Some(server) = server.upgrade() else {
                return;
            };
            server.publish_now(what);
        }
    });
}

impl<W: Wiring + 'static> Server<W> {
    /// Asks the publisher thread to publish.
    pub(super) fn publish(&self, what: Publish) {
        let _ = lock(&self.publisher).send(what);
    }

    /// Republishes every open document (a pass stored findings, a dismissal).
    pub(super) fn publish_open(&self) {
        self.publish(Publish::AllOpen);
    }

    fn publish_now(&self, what: Publish) {
        match what {
            Publish::Clear(path) => {
                lock(&self.published).remove(&path);
                self.client.publish(&path, Vec::new());
            }
            Publish::File(path) => self.publish_if_open(&path),
            Publish::AllOpen => {
                for path in self.client.open_paths() {
                    self.publish_if_open(&path);
                }
            }
            Publish::After(then) => then(),
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
            Ok(Some((shown, diagnostics))) => {
                // remembered first: an editor may ask for the actions on a diagnostic as soon
                // as it arrives
                lock(&self.published).insert(path.to_path_buf(), shown);
                self.client.publish(path, diagnostics);
                lock(&self.errors_logged).clear();
            }
            Ok(None) => {}
            Err(e) => self.log_error(&format!("{e:#}")),
        }
    }

    /// What scry shows in the file now, and the same as diagnostics; `None` when the file is
    /// not the project's or the project has no language for it.
    fn diagnostics_for(
        &self,
        path: &Path,
    ) -> anyhow::Result<Option<(Vec<Shown>, Vec<Diagnostic>)>> {
        let project = (self.cfg.open)(&self.root)?;
        let Some(file) = project_file(&project, path) else {
            return Ok(None);
        };
        if !self.trusted(&project) {
            return Ok(None);
        }
        let store = JsonFindingStore::at(project.state.join("scry/findings.json"));
        let settings = ScryToml::for_project(&project);
        let root = project.root.clone();
        let view = FilesView::new(Arc::new(project), self.cfg.cache.clone());
        let shown = shown_in_file(&store, &settings, &view, &file)?;
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let related = self.client.related_information();
        let diagnostics = shown
            .iter()
            .map(|s| diagnostic(s, &root, &text, related))
            .collect();
        Ok(Some((shown, diagnostics)))
    }

    /// Logs an error to the editor's log for the server, unless it was logged since the
    /// server last published: a broken config is said once, not at every save.
    pub(super) fn log_error(&self, text: &str) {
        if lock(&self.errors_logged).insert(text.to_string()) {
            self.client.log(&format!("scry: {text}"));
        }
    }
}

/// The path relative to the project's root, when the file is the project's (the nearest
/// project root above it is this project's) and the project has a language for it. It is
/// written with `/` on every platform (`src/orders.py`, never `src\orders.py`), as git names
/// files, so the store and the run log read the same whichever route found the file.
///
/// The editor's path may differ from the canonical one (a symlinked directory, `/var` and
/// `/private/var`), so the comparison is made on the canonical path.
pub(super) fn project_file(project: &Project, path: &Path) -> Option<PathBuf> {
    let canonical = canonical(path).unwrap_or_else(|_| path.to_path_buf());
    let directory = canonical.parent()?;
    if Project::root_above(directory)? != project.root {
        return None;
    }
    let relative = PathBuf::from(forward_slashes(canonical.strip_prefix(&project.root).ok()?));
    project.languages.for_path(&relative)?;
    Some(relative)
}
