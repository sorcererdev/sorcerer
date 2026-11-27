//! A conjured change in the host's terms: each changed file as a `FileChange`, so the host's
//! landing (`sorcerer_lsp::landing`: the `WorkspaceEdit`, the moved-on check, the undo edit)
//! lands it. Nothing is written to disk by the server.

use sorcerer_lsp::landing::FileChange;

use crate::usecases::conjure_change::ConjuredChange;

pub(super) use sorcerer_lsp::landing::{
    NewFiles, OpenDocument, Snapshot, moved_on, undo_edit, versioned_text_now, workspace_edit,
};

/// The change's files, in its order, as the host lands them.
pub(super) fn file_changes(change: &ConjuredChange) -> Vec<FileChange<'_>> {
    change
        .files
        .iter()
        .map(|file| FileChange {
            path: &file.path,
            new: file.new,
            before: file.before.as_deref(),
            after: &file.after,
            edits: &file.edits,
        })
        .collect()
}
