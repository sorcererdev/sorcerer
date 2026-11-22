//! Conjuring a change in the editor, landing it, and its life after.

use std::path::PathBuf;
use std::sync::Arc;

use lsp_server::ResponseError;
use lsp_types::MessageType;
use sorcerer_core::domain::paths::forward_slashes;

use super::commands::Ask;
use super::{NOT_TRUSTED, Server};

/// The change marked now.
pub(super) struct Shown {
    pub change: LandedChange,
}

/// A change that landed in the editor.
pub(super) struct LandedChange {
    pub id: String,
    pub files: Vec<LandedFile>,
}

/// One file of a landed change.
pub(super) struct LandedFile {
    /// Relative to the project's root.
    pub path: PathBuf,
    /// As the editor names it.
    pub editor_path: PathBuf,
}

impl LandedFile {
    /// The path relative to the root, written with `/`.
    pub fn relative_text(&self) -> String {
        forward_slashes(&self.path)
    }

    /// The file's name, as a mark says it: `cart__test.py`.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.relative_text())
    }
}

impl Server {
    pub(super) fn conjure(self: &Arc<Self>, ask: Ask) {
        let Ok(project) = (self.cfg.open)(&self.root) else {
            return;
        };
        if !self.trusted(&project) {
            self.client.say(MessageType::WARNING, NOT_TRUSTED);
            return;
        }
        let _ = ask;
    }

    pub(super) fn undo(self: &Arc<Self>, _id: &str) {}

    pub(super) fn try_harder(self: &Arc<Self>, _id: &str) {}

    pub(super) fn show_why(&self, _id: &str) -> Result<(), ResponseError> {
        Ok(())
    }

    pub(super) fn show_file(&self, _id: &str, _path: &str) -> Result<(), ResponseError> {
        Ok(())
    }

    pub(super) fn republish(&self, _path: &std::path::Path) {}

    pub(super) fn changed(&self, _path: &std::path::Path) {}

    pub(super) fn saved(&self, _path: &std::path::Path) {}

    pub(super) fn shutdown(&self) {}
}
