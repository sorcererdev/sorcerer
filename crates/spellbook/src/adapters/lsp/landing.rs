//! A plan's changed files landed by the editor: one `workspace/applyEdit` the person can undo
//! (one step per document), tied to the document version the editor gave for the fix. Only
//! when the editor says it applied the edit are the new files written and the plan recorded.

use std::path::Path;
use std::sync::Arc;

use lsp_types::{
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range,
    TextDocumentEdit, TextEdit, WorkspaceEdit,
};
use sorcerer_lsp::{Reply, uri_of};

use super::commands::stop;
use super::plans::KeptPlan;
use super::server_state::Landing;
use super::{Server, lock};
use crate::domain::change::TextChange;
use crate::domain::plan::{Plan, PlannedFile};
use crate::usecases::apply_plan::ChangesLanded;

/// What an editor that cannot apply edits is told, once: the changed files went to the disk.
pub(super) const CANNOT_EDIT: &str =
    "spellbook: your editor cannot apply edits; changed files were written to disk";

impl Server {
    /// Sends the plan's changes as one edit labelled with the fix (or the generator) and answers
    /// the apply when the editor answers: applied, the new files are written, the plan recorded,
    /// the marks published and the first hole opened; refused (the document moved on), nothing
    /// is written and the person is told to fix again.
    pub(super) fn land_in_the_editor(self: &Arc<Self>, kept: KeptPlan, reply: Reply) {
        let edit = workspace_edit(&self.root, &kept.plan, kept.version);
        let label = edit_label(&kept.plan);
        self.remember_landing(&kept.plan);
        let server = Arc::clone(self);
        let plan = kept.plan;
        self.client.apply_edit(&label, edit, move |answer| {
            let said = if !answer.applied {
                server.forget_refused_landing(&plan);
                stop(&changed_since_text(&plan))
            } else {
                match server.wired_for_commands() {
                    Ok(wired) => server.applied(&wired, plan, ChangesLanded::ByTheEditor),
                    Err(sentence) => stop(&sentence),
                }
            };
            reply.send(Ok(said));
        });
    }
}

impl Server {
    /// Keeps the text the plan leaves in each changed file, so a file the editor opens to land
    /// the edit is marked from it, not from the disk.
    fn remember_landing(&self, plan: &Plan) {
        let mut landing = lock(&self.landing);
        for file in plan.changed_files() {
            landing.insert(
                self.root.join(&file.path),
                Landing {
                    file: file.path.clone(),
                    text: file.text.clone(),
                },
            );
        }
    }

    /// The editor refused the edit: the changed files are the disk's again, and those open in
    /// the editor are marked from it.
    fn forget_refused_landing(&self, plan: &Plan) {
        let paths: Vec<_> = plan
            .changed_files()
            .into_iter()
            .map(|file| self.root.join(&file.path))
            .collect();
        lock(&self.landing).retain(|path, _| !paths.contains(path));
        let open: Vec<_> = paths
            .into_iter()
            .filter(|path| lock(&self.published).contains_key(path))
            .collect();
        for path in open {
            self.republish(&path);
        }
    }
}

/// The edit that lands a plan's changed files: one document edit per file, in the plan's order.
/// The fix's own file carries the version the editor gave, so the editor refuses it if the
/// person typed since; the other files carry none.
fn workspace_edit(root: &Path, plan: &Plan, version: Option<i32>) -> WorkspaceEdit {
    let fixed_file = plan.fix.as_ref().map(|fix| fix.file.as_str());
    let documents = plan
        .changed_files()
        .into_iter()
        .map(|file| TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: uri_of(&root.join(&file.path)),
                version: version.filter(|_| fixed_file == Some(file.path.as_str())),
            },
            edits: text_edits(file).into_iter().map(OneOf::Left).collect(),
        })
        .collect();
    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Edits(documents)),
        ..WorkspaceEdit::default()
    }
}

/// A changed file's changes as text edits over the file as the plan saw it.
fn text_edits(file: &PlannedFile) -> Vec<TextEdit> {
    file.changes
        .iter()
        .map(|change| text_edit(&file.before, change))
        .collect()
}

/// One whole-line change as a text edit. Its rows become line starts (column 0, which is the
/// same in UTF-16), except past the last line of a text that does not end with a line ending:
/// there the edit starts at the end of that last line and puts the line ending first, as
/// `apply_changes` does.
///
/// Example: in `a\nb` (no line ending at the end), inserting `c\n` at row 2 is the edit at
/// 1:1..1:1 with the text `\nc\n`.
fn text_edit(before: &str, change: &TextChange) -> TextEdit {
    let rows: Vec<&str> = before.split_inclusive('\n').collect();
    let open_last_row = rows.last().filter(|row| !row.ends_with('\n'));
    let position = |row: usize| match open_last_row {
        Some(last) if row >= rows.len() => Position::new(
            u32::try_from(rows.len() - 1).unwrap_or(u32::MAX),
            u32::try_from(last.encode_utf16().count()).unwrap_or(u32::MAX),
        ),
        _ => Position::new(u32::try_from(row).unwrap_or(u32::MAX), 0),
    };
    let new_text = match open_last_row {
        Some(_) if change.from_row >= rows.len() => {
            let ending = if before.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            format!("{ending}{}", change.new)
        }
        _ => change.new.clone(),
    };
    TextEdit {
        range: Range::new(position(change.from_row), position(change.to_row)),
        new_text,
    }
}

/// The label the editor may show for the edit (in its undo history, say).
///
/// Example: `spellbook: Use a port the service has`; for a `new` plan, `spellbook: command`.
fn edit_label(plan: &Plan) -> String {
    let named = plan
        .fix
        .as_ref()
        .map_or(plan.generator.as_str(), |fix| fix.label.as_str());
    format!("spellbook: {named}")
}

/// What a refused edit is told: the file (the fix's, else the first changed one) changed since
/// the plan, and nothing was applied.
///
/// Example: `close_table.py changed since the plan; nothing was applied: fix again`.
pub(super) fn changed_since_text(plan: &Plan) -> String {
    let path = plan
        .fix
        .as_ref()
        .map(|fix| fix.file.clone())
        .or_else(|| plan.changed_files().first().map(|file| file.path.clone()));
    let name = path
        .as_deref()
        .map(|path| path.rsplit('/').next().unwrap_or(path).to_string())
        .unwrap_or_default();
    let again = if plan.fix.is_some() {
        "fix again"
    } else {
        "plan it again"
    };
    format!("{name} changed since the plan; nothing was applied: {again}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::change::changes_between;

    /// The text with one file's edits made, the ranges counted in the text as it was.
    fn made(before: &str, after: &str) -> String {
        let offset = |position: Position| {
            before
                .split_inclusive('\n')
                .take(position.line as usize)
                .map(str::len)
                .sum::<usize>()
                + position.character as usize
        };
        let mut edits: Vec<TextEdit> = changes_between(before, after)
            .iter()
            .map(|change| text_edit(before, change))
            .collect();
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.start));
        let mut text = before.to_string();
        for edit in edits {
            text.replace_range(
                offset(edit.range.start)..offset(edit.range.end),
                &edit.new_text,
            );
        }
        text
    }

    #[test]
    fn the_edits_turn_the_text_into_the_planned_one() {
        for (before, after) in [
            ("a\nb\nc\n", "a\nB\nc\nd\n"),
            ("a\nb\n", "z\na\nb\n"),
            ("a\nb", "a\nb\nc\n"),
            ("a\nb", "a\nc\n"),
            ("a\r\nb\r\n", "a\r\nc\r\nb\r\n"),
            ("", "a\n"),
        ] {
            assert_eq!(made(before, after), after, "{before:?} to {after:?}");
        }
    }

    #[test]
    fn an_insertion_past_an_open_last_line_puts_the_line_ending_first() {
        let change = TextChange {
            from_row: 2,
            to_row: 2,
            old: String::new(),
            new: "c\n".into(),
        };
        let edit = text_edit("a\nb", &change);
        assert_eq!(
            edit.range,
            Range::new(Position::new(1, 1), Position::new(1, 1))
        );
        assert_eq!(edit.new_text, "\nc\n");
    }
}
