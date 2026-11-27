//! The lines Sorcerer's own servers asked the editor to land, noted in the project's state so a
//! server that watches the person's typing can tell them apart. The editor reports every change
//! the same way (`textDocument/didChange` with the whole text), whoever made it; a line that
//! spellbook's sync, a conjure step or an apprentice task landed a moment ago is theirs, not
//! the person's.
//!
//! The note is `.sorcerer/state/tool-edits.jsonl`: one line per edit sent, with the time, the
//! file and the lines it puts there. Every server writes it before it sends the edit, so the
//! note is there by the time the editor reports the change. A note is read for ten minutes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lsp_types::{DocumentChangeOperation, DocumentChanges, OneOf, TextDocumentEdit, WorkspaceEdit};
use serde::{Deserialize, Serialize};

use crate::uri::path_of;

/// The note's file name under the state directory.
pub const NOTE_FILE: &str = "tool-edits.jsonl";

/// How long a noted line counts as a tool's: long enough for the person to stop typing and
/// for the idle look, or the save, that follows.
pub const NOTED_FOR: Duration = Duration::from_secs(600);

/// When the note holds more lines than this, writing one more drops the ones too old to count.
const PRUNE_ABOVE: usize = 200;

/// One edit sent: when, which file (as the editor names it), and the lines it puts there,
/// trimmed, blank ones left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Noted {
    at: u64,
    file: PathBuf,
    lines: Vec<String>,
}

/// Notes the lines an edit puts in each file, before the edit is sent. A note that cannot be
/// written is skipped: the edit still goes, and its lines are then triaged as anyone's.
///
/// Example: spellbook's sync inserting `mod clock` into its module list `src/ports/list.txt` notes
/// the file with the one line `mod clock`.
pub fn note_tool_edit(state_dir: &Path, edit: &WorkspaceEdit, at: SystemTime) {
    let seconds = seconds_of(at);
    let noted: Vec<Noted> = lines_by_file(edit)
        .into_iter()
        .filter(|(_, lines)| !lines.is_empty())
        .map(|(file, lines)| Noted {
            at: seconds,
            file,
            lines,
        })
        .collect();
    if noted.is_empty() {
        return;
    }
    let _ = append(state_dir, &noted, seconds);
}

/// The lines a tool landed in a file in the last ten minutes, trimmed, in the order they were
/// noted (each edit's lines in file order).
///
/// Example: after the note above, `lines_tools_landed(state, ".../src/ports/list.txt", now)`
/// holds `mod clock`; for any other file it is empty.
pub fn lines_tools_landed(state_dir: &Path, file: &Path, now: SystemTime) -> Vec<String> {
    let oldest = seconds_of(now).saturating_sub(NOTED_FOR.as_secs());
    read(state_dir)
        .into_iter()
        .filter(|noted| noted.at >= oldest && noted.file == file)
        .flat_map(|noted| noted.lines)
        .collect()
}

/// Each edited file of the edit with the lines its text edits put there, whichever of the
/// protocol's three shapes the edit takes (`documentChanges` as edits only, or with file
/// operations, or the plain `changes` map).
fn lines_by_file(edit: &WorkspaceEdit) -> Vec<(PathBuf, Vec<String>)> {
    let documents: Vec<&TextDocumentEdit> = match &edit.document_changes {
        Some(DocumentChanges::Edits(edits)) => edits.iter().collect(),
        Some(DocumentChanges::Operations(operations)) => operations
            .iter()
            .filter_map(|operation| match operation {
                DocumentChangeOperation::Edit(edit) => Some(edit),
                DocumentChangeOperation::Op(_) => None,
            })
            .collect(),
        None => Vec::new(),
    };
    let from_documents = documents.into_iter().filter_map(|document| {
        let file = path_of(&document.text_document.uri)?;
        let lines = document
            .edits
            .iter()
            .flat_map(|text_edit| match text_edit {
                OneOf::Left(text_edit) => non_blank_lines(&text_edit.new_text),
                OneOf::Right(annotated) => non_blank_lines(&annotated.text_edit.new_text),
            })
            .collect();
        Some((file, lines))
    });
    let from_changes = edit.changes.iter().flatten().filter_map(|(uri, edits)| {
        let lines = edits
            .iter()
            .flat_map(|e| non_blank_lines(&e.new_text))
            .collect();
        Some((path_of(uri)?, lines))
    });
    from_documents.chain(from_changes).collect()
}

fn non_blank_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn append(state_dir: &Path, noted: &[Noted], now: u64) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let path = state_dir.join(NOTE_FILE);
    let kept = read(state_dir);
    if kept.len() > PRUNE_ABOVE {
        let oldest = now.saturating_sub(NOTED_FOR.as_secs());
        let young: Vec<Noted> = kept.into_iter().filter(|n| n.at >= oldest).collect();
        std::fs::write(&path, jsonl(&young))?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?
        .write_all(jsonl(noted).as_bytes())
}

fn read(state_dir: &Path) -> Vec<Noted> {
    std::fs::read_to_string(state_dir.join(NOTE_FILE))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn jsonl(noted: &[Noted]) -> String {
    noted
        .iter()
        .filter_map(|n| serde_json::to_string(n).ok())
        .map(|line| line + "\n")
        .collect()
}

fn seconds_of(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landing::{FileChange, NewFiles, Snapshot, workspace_edit};
    use crate::line_edits::line_edits;
    use std::collections::BTreeMap;

    #[test]
    fn a_landed_line_is_a_tools_for_ten_minutes_in_its_file_only() {
        let state = tempfile::TempDir::new().unwrap();
        let root = state.path().join("project");
        let before = "mod booking\n";
        let after = "mod booking\nmod clock\n";
        let edits = line_edits(before, after);
        let change = FileChange {
            path: Path::new("src/ports/list.txt"),
            new: false,
            before: Some(before),
            after,
            edits: &edits,
        };
        let snapshot = Snapshot {
            root: root.clone(),
            open: BTreeMap::new(),
        };
        let (edit, _) = workspace_edit(&[change], &snapshot, NewFiles::Created);
        let then = UNIX_EPOCH + Duration::from_secs(1_000_000);
        note_tool_edit(state.path(), &edit, then);

        let file = std::path::absolute(root.join("src/ports/list.txt")).unwrap();
        let landed = lines_tools_landed(state.path(), &file, then + Duration::from_secs(5));
        assert_eq!(landed, ["mod clock"]);
        let other = std::path::absolute(root.join("src/other.txt")).unwrap();
        assert!(lines_tools_landed(state.path(), &other, then).is_empty());
        let later = then + NOTED_FOR + Duration::from_secs(1);
        assert!(lines_tools_landed(state.path(), &file, later).is_empty());
    }

    #[test]
    fn an_edit_of_documents_only_is_noted_too() {
        let state = tempfile::TempDir::new().unwrap();
        let file = std::path::absolute(state.path().join("a.txt")).unwrap();
        let edit = WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                    uri: crate::uri::uri_of(&file),
                    version: Some(3),
                },
                edits: vec![OneOf::Left(lsp_types::TextEdit {
                    range: lsp_types::Range::default(),
                    new_text: "    use the port\n\n".into(),
                })],
            }])),
            ..WorkspaceEdit::default()
        };
        let now = SystemTime::now();
        note_tool_edit(state.path(), &edit, now);
        let landed = lines_tools_landed(state.path(), &file, now);
        assert_eq!(landed, ["use the port"]);
    }
}
