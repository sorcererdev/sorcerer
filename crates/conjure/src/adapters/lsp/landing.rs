//! A change as one `WorkspaceEdit`: each open file edited at the version its text was read at,
//! each file not open edited with no version (the editor opens it as an unsaved buffer), each
//! new file created and then filled. Nothing is written to disk by the server.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lsp_types::{
    CreateFile, CreateFileOptions, DeleteFile, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, ResourceOp, TextDocumentEdit,
    TextEdit, WorkspaceEdit,
};
use sorcerer_lsp::uri_of;

use crate::domain::edits::LineEdit;
use crate::usecases::conjure_change::{ChangedFile, ConjuredChange};

/// The open documents of the project at the moment a change was asked for: each one's path as
/// the editor names it, its version and its text, by path relative to the root.
pub(super) struct Snapshot {
    pub root: PathBuf,
    pub open: BTreeMap<PathBuf, OpenDocument>,
    /// When the documents were read: a file not open that the disk shows written later has
    /// moved on.
    pub read_at: std::time::SystemTime,
}

/// An open document as the editor last sent it.
pub(super) struct OpenDocument {
    pub editor_path: PathBuf,
    pub version: i32,
    pub text: String,
}

impl Snapshot {
    /// The open documents' text, by path relative to the root, for the editor's view.
    pub fn texts(&self) -> BTreeMap<PathBuf, String> {
        self.open
            .iter()
            .map(|(path, document)| (path.clone(), document.text.clone()))
            .collect()
    }

    /// The version a file's text was read at; `None` for a file that was not open.
    pub fn version_of(&self, relative: &Path) -> Option<i32> {
        self.open.get(relative).map(|document| document.version)
    }

    /// The path the editor names a file with: its own for an open file, else under the root.
    pub fn editor_path(&self, relative: &Path) -> PathBuf {
        self.open
            .get(relative)
            .map_or_else(|| self.root.join(relative), |d| d.editor_path.clone())
    }
}

/// Whether the edit may create files: the editor said it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NewFiles {
    Created,
    /// The editor cannot create files: new files are left out of the edit.
    LeftOut,
}

/// The change as one edit, and the new files left out of it (when the editor cannot create
/// files). The files come in the change's order.
///
/// Example: a change of the open `a.py` (version 2), the unopened `b.py` and a new
/// `a__test.py` is an edit of `a.py` at version 2, an edit of `b.py` with no version, then a
/// `CreateFile` of `a__test.py` and an edit inserting its text.
pub(super) fn workspace_edit(
    change: &ConjuredChange,
    snapshot: &Snapshot,
    new_files: NewFiles,
) -> (WorkspaceEdit, Vec<PathBuf>) {
    let (operations, left_out) = change.files.iter().fold(
        (Vec::new(), Vec::new()),
        |(mut operations, mut left_out), file| {
            match (file.new, new_files) {
                (true, NewFiles::LeftOut) => left_out.push(file.path.clone()),
                (true, NewFiles::Created) => operations.extend(created(file, snapshot)),
                (false, _) => operations.push(edited(file, snapshot)),
            }
            (operations, left_out)
        },
    );
    (edit_of(operations), left_out)
}

/// An existing file's edits, at the version its text was read at.
fn edited(file: &ChangedFile, snapshot: &Snapshot) -> DocumentChangeOperation {
    text_document_edit(
        &snapshot.editor_path(&file.path),
        snapshot.version_of(&file.path),
        &file.edits,
    )
}

/// A new file: created (left alone if it is there by then), then its text inserted.
fn created(file: &ChangedFile, snapshot: &Snapshot) -> [DocumentChangeOperation; 2] {
    let path = snapshot.editor_path(&file.path);
    let insert = LineEdit {
        start_line: 0,
        end_line: 0,
        text: file.after.clone(),
        to_end_of_file: None,
    };
    [
        DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
            uri: uri_of(&path),
            options: Some(CreateFileOptions {
                overwrite: None,
                ignore_if_exists: Some(true),
            }),
            annotation_id: None,
        })),
        text_document_edit(&path, None, &[insert]),
    ]
}

/// What undoing one file takes.
pub(super) enum Undoing<'a> {
    /// The file's edits back to its text before, at the version it has now.
    Edit {
        path: &'a Path,
        version: Option<i32>,
        edits: Vec<LineEdit>,
    },
    /// A file the change created, deleted.
    Delete { path: &'a Path },
}

/// The undo of a change as one edit.
pub(super) fn undo_edit(undoings: Vec<Undoing>) -> WorkspaceEdit {
    edit_of(
        undoings
            .into_iter()
            .map(|undoing| match undoing {
                Undoing::Edit {
                    path,
                    version,
                    edits,
                } => text_document_edit(path, version, &edits),
                Undoing::Delete { path } => {
                    DocumentChangeOperation::Op(ResourceOp::Delete(DeleteFile {
                        uri: uri_of(path),
                        options: None,
                    }))
                }
            })
            .collect(),
    )
}

fn edit_of(operations: Vec<DocumentChangeOperation>) -> WorkspaceEdit {
    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Operations(operations)),
        ..WorkspaceEdit::default()
    }
}

fn text_document_edit(
    path: &Path,
    version: Option<i32>,
    edits: &[LineEdit],
) -> DocumentChangeOperation {
    DocumentChangeOperation::Edit(TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier {
            uri: uri_of(path),
            version,
        },
        edits: edits
            .iter()
            .map(|edit| OneOf::Left(text_edit(edit)))
            .collect(),
    })
}

/// A whole-line edit in the protocol's terms: from the start of its first line to the start of
/// the line after its last, or, when it reaches the end of a file whose last line has no
/// newline, to the end of that line (in UTF-16 units, the protocol's default).
fn text_edit(edit: &LineEdit) -> TextEdit {
    let end = match edit.to_end_of_file {
        Some(length) => Position::new(edit.end_line.saturating_sub(1), length),
        None => Position::new(edit.end_line, 0),
    };
    TextEdit {
        range: Range {
            start: Position::new(edit.start_line, 0),
            end,
        },
        new_text: edit.text.clone(),
    }
}
