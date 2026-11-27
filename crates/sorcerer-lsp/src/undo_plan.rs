//! Taking one landed file back: from its text now, whether it can go back to its text before,
//! needs nothing, or holds the person's edits and must be left alone. A product plans every
//! file of a change this way and sends the undoings as one edit ([`crate::landing::undo_edit`]).

use std::path::{Path, PathBuf};

use crate::landing::Undoing;
use crate::line_edits::{LineEdit, in_base_line_endings, line_edits, same_text};

/// One file a change landed: as the editor names it, whether the change created it, and its
/// text before (none for a new file) and after.
#[derive(Debug, Clone, Copy)]
pub struct TakenBack<'a> {
    pub editor_path: &'a Path,
    pub new: bool,
    pub before: Option<&'a str>,
    pub after: &'a str,
}

/// Whether the editor may delete a file the change created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDeletion {
    Allowed,
    NotAllowed,
}

/// One file's undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    /// Edits back to the text before, at the version the file has now.
    Edit {
        path: PathBuf,
        version: Option<i32>,
        edits: Vec<LineEdit>,
    },
    /// A new file emptied (the editor cannot delete files).
    Empty {
        path: PathBuf,
        version: Option<i32>,
        edits: Vec<LineEdit>,
    },
    Delete {
        path: PathBuf,
    },
}

impl Undo {
    /// The undo as one part of an edit.
    pub fn undoing(&self) -> Undoing<'_> {
        match self {
            Undo::Edit {
                path,
                version,
                edits,
            }
            | Undo::Empty {
                path,
                version,
                edits,
            } => Undoing::Edit {
                path,
                version: *version,
                edits: edits.clone(),
            },
            Undo::Delete { path } => Undoing::Delete { path },
        }
    }
}

/// What undoing one file takes, from its text now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileUndo {
    Undo(Undo),
    /// The file is already as it was (the edit never reached it, or the person put it back).
    Nothing,
    /// The person edited it after the change landed.
    Edited,
}

/// One file's undo, from its version and text now (the editor's, else the disk's):
///
/// - at the change's after-text: back to its text before (a new file deleted, or emptied when
///   the editor cannot delete);
/// - already at its text before: nothing;
/// - a new file closed unsaved, so empty on disk (the editor created it empty): deleted when
///   the editor can;
/// - anything else is the person's, and is never overwritten.
///
/// Example: `a.py` landed as `x = 2` over `x = 1`; with `x = 2` in the buffer at version 7 its
/// undo is one edit at version 7 back to `x = 1`; with `x = 3` it is `Edited`.
pub fn undo_of(
    file: &TakenBack,
    now: Option<(Option<i32>, String)>,
    deletion: FileDeletion,
) -> FileUndo {
    let Some((version, text)) = now else {
        // in neither the editor nor the disk: only a new file may be gone
        return if file.new {
            FileUndo::Nothing
        } else {
            FileUndo::Edited
        };
    };
    let path = file.editor_path.to_path_buf();
    let before = file.before.unwrap_or("");
    if !same_text(&text, file.after) {
        return match (file.new, version, deletion) {
            (true, None, FileDeletion::Allowed) if text.is_empty() => {
                FileUndo::Undo(Undo::Delete { path })
            }
            _ if text == before => FileUndo::Nothing,
            _ => FileUndo::Edited,
        };
    }
    match (file.before, deletion) {
        (Some(before), _) => FileUndo::Undo(Undo::Edit {
            path,
            version,
            // in the file's own line endings: an undo does not rewrite them
            edits: line_edits(&text, &in_base_line_endings(&text, before)),
        }),
        (None, FileDeletion::Allowed) => FileUndo::Undo(Undo::Delete { path }),
        (None, FileDeletion::NotAllowed) => FileUndo::Undo(Undo::Empty {
            path,
            version,
            edits: line_edits(&text, ""),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(before: Option<&'a str>, after: &'a str) -> TakenBack<'a> {
        TakenBack {
            editor_path: Path::new("a.txt"),
            new: before.is_none(),
            before,
            after,
        }
    }

    #[test]
    fn a_file_at_its_after_text_goes_back_at_its_version() {
        let undo = undo_of(
            &file(Some("x = 1\n"), "x = 2\n"),
            Some((Some(7), "x = 2\n".into())),
            FileDeletion::Allowed,
        );
        let FileUndo::Undo(Undo::Edit { version, edits, .. }) = undo else {
            panic!("{undo:?}");
        };
        assert_eq!(version, Some(7));
        assert_eq!(edits[0].text, "x = 1\n");
    }

    #[test]
    fn a_file_the_person_edited_is_left_alone() {
        let undo = undo_of(
            &file(Some("x = 1\n"), "x = 2\n"),
            Some((Some(8), "x = 3\n".into())),
            FileDeletion::Allowed,
        );
        assert_eq!(undo, FileUndo::Edited);
        let back = undo_of(
            &file(Some("x = 1\n"), "x = 2\n"),
            Some((Some(9), "x = 1\n".into())),
            FileDeletion::Allowed,
        );
        assert_eq!(back, FileUndo::Nothing);
    }

    #[test]
    fn a_new_file_is_deleted_or_emptied() {
        let deleted = undo_of(
            &file(None, "new\n"),
            Some((Some(1), "new\n".into())),
            FileDeletion::Allowed,
        );
        assert!(matches!(deleted, FileUndo::Undo(Undo::Delete { .. })));
        let emptied = undo_of(
            &file(None, "new\n"),
            Some((Some(1), "new\n".into())),
            FileDeletion::NotAllowed,
        );
        assert!(matches!(emptied, FileUndo::Undo(Undo::Empty { .. })));
        let gone = undo_of(&file(None, "new\n"), None, FileDeletion::Allowed);
        assert_eq!(gone, FileUndo::Nothing);
    }
}
