//! Landing a change in the editor: the change as one `WorkspaceEdit` (each open file edited at
//! the version its text was read at, each file not open edited with no version, each new file
//! created and then filled), whether a file moved on since its text was read, and the undo of a
//! change as one edit. Nothing is written to disk here; the editor applies the edit.
//!
//! conjure lands its one change this way, and lackey each `write_file` of a task.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lsp_types::{
    CreateFile, CreateFileOptions, DeleteFile, DeleteFileOptions, DocumentChangeOperation,
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range, ResourceOp,
    TextDocumentEdit, TextEdit, WorkspaceEdit,
};

use crate::client::Client;
use crate::line_edits::LineEdit;
use crate::uri::uri_of;

/// The open documents of the project at the moment a change was asked for: each one's path as
/// the editor names it, its version and its text, by path relative to the root.
pub struct Snapshot {
    pub root: PathBuf,
    pub open: BTreeMap<PathBuf, OpenDocument>,
}

/// An open document as the editor last sent it.
pub struct OpenDocument {
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

/// One file of a change to land: its path relative to the root, whether the change creates it,
/// its text before (none for a new file) and after, and the edits from one to the other.
#[derive(Debug, Clone, Copy)]
pub struct FileChange<'a> {
    pub path: &'a Path,
    pub new: bool,
    pub before: Option<&'a str>,
    pub after: &'a str,
    pub edits: &'a [LineEdit],
}

/// Whether the edit may create files: the editor said it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewFiles {
    Created,
    /// The editor cannot create files: new files are left out of the edit.
    LeftOut,
}

/// The change as one edit, and the new files left out of it (when the editor cannot create
/// files). The files come in the change's order.
///
/// Example: a change of the open `a.py` (version 2), the unopened `b.py` and a new
/// `a__test.py` is an edit of `a.py` at version 2, an edit of `b.py` with no version, then a
/// `CreateFile` of `a__test.py` and an edit writing its text over the new, empty buffer.
pub fn workspace_edit(
    files: &[FileChange],
    snapshot: &Snapshot,
    new_files: NewFiles,
) -> (WorkspaceEdit, Vec<PathBuf>) {
    let (operations, left_out) = files.iter().fold(
        (Vec::new(), Vec::new()),
        |(mut operations, mut left_out), file| {
            match (file.new, new_files) {
                (true, NewFiles::LeftOut) => left_out.push(file.path.to_path_buf()),
                (true, NewFiles::Created) => operations.extend(created(file, snapshot)),
                (false, _) => operations.push(edited(file, snapshot)),
            }
            (operations, left_out)
        },
    );
    (edit_of(operations), left_out)
}

/// Whether a file the change edits moved on since its text was read: an open file at another
/// version; a file that was not open and is open now, or on disk, with other text; a new file
/// that is there now. The editor would refuse the first (most do; Neovim checks only the first
/// file of an edit) and would apply the others over what is there, so a product sends none.
pub fn moved_on(client: &Client, snapshot: &Snapshot, files: &[FileChange]) -> bool {
    files.iter().any(|file| {
        let path = snapshot.editor_path(file.path);
        let now = client.text_of(&path);
        match (snapshot.version_of(file.path), now) {
            (Some(read), Some((version, _))) => version != read,
            (Some(_), None) => true,
            (None, Some((_, text))) => Some(text.as_str()) != file.before,
            // not open: the disk is not as the change read it
            (None, None) if file.new => path.exists(),
            (None, None) => std::fs::read_to_string(&path).ok().as_deref() != file.before,
        }
    })
}

/// A file's version and text as the editor has it (with its version), else as the disk has it
/// (no version); `None` when it is in neither.
pub fn versioned_text_now(client: &Client, path: &Path) -> Option<(Option<i32>, String)> {
    match client.text_of(path) {
        Some((version, text)) => Some((Some(version), text)),
        None => std::fs::read_to_string(path).ok().map(|text| (None, text)),
    }
}

/// An existing file's edits, at the version its text was read at.
fn edited(file: &FileChange, snapshot: &Snapshot) -> DocumentChangeOperation {
    text_document_edit(
        &snapshot.editor_path(file.path),
        snapshot.version_of(file.path),
        file.edits,
    )
}

/// A new file: created (left alone if it is there by then), then its text written over the
/// whole of the new buffer.
///
/// The text replaces lines 0 up to 1, not an insertion at the start: a new Neovim buffer
/// already holds one empty line, so inserting `a\nb\n` at 0:0 leaves `a\nb\n` and that empty
/// line (`a\nb\n\n` with `eol`), which is not the text sent. An edit that reaches past the last
/// line is one Neovim fits to the buffer and then drops the empty last line of; VS Code fits it
/// to its empty document the same way. Both then hold exactly `a\nb\n`.
fn created(file: &FileChange, snapshot: &Snapshot) -> [DocumentChangeOperation; 2] {
    let path = snapshot.editor_path(file.path);
    let whole_buffer = LineEdit {
        start_line: 0,
        end_line: 1,
        text: file.after.to_string(),
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
        text_document_edit(&path, None, &[whole_buffer]),
    ]
}

/// What undoing one file takes.
pub enum Undoing<'a> {
    /// The file's edits back to its text before, at the version it has now.
    Edit {
        path: &'a Path,
        version: Option<i32>,
        edits: Vec<LineEdit>,
    },
    /// A file the change created, deleted (whether or not it was ever saved).
    Delete { path: &'a Path },
}

/// The undo of a change as one edit.
pub fn undo_edit(undoings: Vec<Undoing>) -> WorkspaceEdit {
    edit_of(
        undoings
            .into_iter()
            .map(|undoing| match undoing {
                Undoing::Edit {
                    path,
                    version,
                    edits,
                } => text_document_edit(path, version, &edits),
                // a created file Neovim holds only as a buffer is not on disk until saved
                Undoing::Delete { path } => {
                    DocumentChangeOperation::Op(ResourceOp::Delete(DeleteFile {
                        uri: uri_of(path),
                        options: Some(DeleteFileOptions {
                            recursive: None,
                            ignore_if_not_exists: Some(true),
                            annotation_id: None,
                        }),
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
