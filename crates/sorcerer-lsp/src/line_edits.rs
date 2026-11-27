//! A file's new text as whole-line edits against its old text, and the line-ending rules an
//! edit keeps: what every product that lands an edit through the editor needs.

use std::ops::Range;

use similar::{DiffTag, TextDiff};

/// One edit of a file: lines `start_line` up to (not including) `end_line`, counted from 0,
/// are replaced by `text`. An insertion has `start_line == end_line`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    /// When the edit reaches the end of a file whose last line has no newline: that line's
    /// length in UTF-16 units, where the edit ends (on line `end_line - 1`).
    pub to_end_of_file: Option<u32>,
}

/// Whether two texts are the same but for their line endings and one final newline: an editor
/// may keep a file with CRLF lines (VS Code on Windows starts a new file so) where the change
/// was made with LF, and may end every buffer with a newline (Neovim's `eol`, VS Code's
/// `files.insertFinalNewline`) where the change ended without one. Neither is anybody's edit.
///
/// Example: `"x = 1"`, `"x = 1\n"` and `"x = 1\r\n"` are the same text; `"x = 1\n\n"` is not,
/// since only one final newline is the editor's.
pub fn same_text(a: &str, b: &str) -> bool {
    a == b
        || without_final_newline(&a.replace("\r\n", "\n"))
            == without_final_newline(&b.replace("\r\n", "\n"))
}

fn without_final_newline(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

/// The new text with the base text's line endings: a base of CRLF lines keeps CRLF, one of LF
/// lines keeps LF. A base with no line ending (or none at all) leaves the text as it is.
pub fn in_base_line_endings(base: &str, new: &str) -> String {
    let crlf = base.matches("\r\n").count();
    let lf_only = base.matches('\n').count() - crlf;
    if crlf == 0 && lf_only == 0 {
        return new.to_string();
    }
    let as_lf = new.replace("\r\n", "\n");
    if crlf > lf_only {
        as_lf.replace('\n', "\r\n")
    } else {
        as_lf
    }
}

/// Keeps a base's missing final newline: the base `a\nb` and the new `A\nb\n` is `A\nb`, so the
/// last line is not edited when the writer did not change it.
pub fn keep_unterminated_last_line(base: &str, new: &str) -> String {
    if base.is_empty() || base.ends_with('\n') {
        return new.to_string();
    }
    let ending = if base.contains("\r\n") { "\r\n" } else { "\n" };
    let Some(without_final_newline) = new.strip_suffix(ending) else {
        return new.to_string();
    };
    if last_line(without_final_newline) == last_line(base) {
        without_final_newline.to_string()
    } else {
        new.to_string()
    }
}

fn last_line(text: &str) -> &str {
    text.rsplit('\n').next().unwrap_or(text)
}

/// What changes in a file, as the fewest whole-line edits: each run of changed lines is one
/// edit, so the editor's gutter moves only where the writer wrote.
pub fn line_edits(base: &str, new: &str) -> Vec<LineEdit> {
    let base_lines: Vec<&str> = base.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let diff = TextDiff::from_lines(base, new);
    changed_runs(&diff)
        .into_iter()
        .map(|run| LineEdit {
            start_line: run.old.start as u32,
            end_line: run.old.end as u32,
            text: new_lines[run.new].concat(),
            to_end_of_file: length_of_unterminated_last_line(&base_lines, run.old.end),
        })
        .collect()
}

/// A stretch of lines of the base that became a stretch of lines of the new text.
struct ChangedRun {
    old: Range<usize>,
    new: Range<usize>,
}

/// The diff's changed stretches, neighbouring ones joined.
fn changed_runs(diff: &TextDiff<'_, '_, str>) -> Vec<ChangedRun> {
    let mut runs: Vec<ChangedRun> = Vec::new();
    let mut open: Option<ChangedRun> = None;
    for op in diff.ops() {
        let (old, new) = (op.old_range(), op.new_range());
        if matches!(op.tag(), DiffTag::Equal) {
            runs.extend(open.take());
            continue;
        }
        open = Some(match open {
            None => ChangedRun { old, new },
            Some(run) => ChangedRun {
                old: run.old.start..old.end,
                new: run.new.start..new.end,
            },
        });
    }
    runs.extend(open);
    runs
}

/// Where an edit that reaches the end of the base ends on its last line, when that line has no
/// newline: its length in UTF-16 units.
fn length_of_unterminated_last_line(base_lines: &[&str], edit_end: usize) -> Option<u32> {
    let last = base_lines.last()?;
    (edit_end == base_lines.len() && !last.ends_with('\n'))
        .then(|| last.encode_utf16().count() as u32)
}

/// The first line (from 1) where the new text differs from the base; none when equal.
pub fn first_changed_line(base: &str, new: &str) -> Option<u32> {
    line_edits(base, new)
        .first()
        .map(|edit| edit.start_line + 1)
}

/// How many lines the edits touch: for each edit the larger of the lines it removes and adds.
pub fn lines_changed(edits: &[LineEdit]) -> u32 {
    edits
        .iter()
        .map(|edit| {
            let removed = edit.end_line - edit.start_line;
            let added = edit.text.split_inclusive('\n').count() as u32;
            removed.max(added)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unterminated_last_line_stays_unterminated_when_only_an_earlier_line_changed() {
        assert_eq!(keep_unterminated_last_line("a\nb", "A\nb\n"), "A\nb");
        assert_eq!(
            keep_unterminated_last_line("a\r\nb", "A\r\nb\r\n"),
            "A\r\nb"
        );
        // the writer changed the last line, or the base ended in a newline: nothing to keep
        assert_eq!(keep_unterminated_last_line("a\nb", "a\nB\n"), "a\nB\n");
        assert_eq!(keep_unterminated_last_line("a\nb\n", "A\nb\n"), "A\nb\n");
        assert_eq!(keep_unterminated_last_line("", "a\n"), "a\n");
    }

    #[test]
    fn an_editors_final_newline_or_line_endings_are_the_same_text() {
        assert!(same_text("x = 1", "x = 1\n"));
        assert!(same_text("x = 1\r\n", "x = 1"));
        assert!(same_text("a\r\nb\r\n", "a\nb\n"));
        assert!(!same_text("x = 1", "x = 1\n\n"));
        assert!(!same_text("x = 1", "x = 2\n"));
        assert!(!same_text("", "\n\n"));
    }

    #[test]
    fn files_equal_to_their_base_are_no_change() {
        assert!(line_edits("a\nb\n", "a\nb\n").is_empty());
        assert!(line_edits("", "").is_empty());
        assert_eq!(first_changed_line("a\n", "a\n"), None);
    }

    #[test]
    fn a_crlf_file_keeps_its_line_endings() {
        let base = "a\r\nb\r\n";
        let new = in_base_line_endings(base, "a\nB\n");
        assert_eq!(new, "a\r\nB\r\n");
        let edits = line_edits(base, &new);
        assert_eq!(
            edits,
            [LineEdit {
                start_line: 1,
                end_line: 2,
                text: "B\r\n".into(),
                to_end_of_file: None
            }]
        );
        assert_eq!(in_base_line_endings("a\nb\n", "a\r\nB\r\n"), "a\nB\n");
        assert_eq!(in_base_line_endings("", "a\nb\n"), "a\nb\n");
    }

    #[test]
    fn an_edit_at_an_unterminated_last_line_ends_in_utf16_units() {
        let edits = line_edits("x\ncafé", "x\ncafés\n");
        assert_eq!(
            edits,
            [LineEdit {
                start_line: 1,
                end_line: 2,
                text: "cafés\n".into(),
                to_end_of_file: Some(4)
            }]
        );
        // an emoji is two UTF-16 units
        let edits = line_edits("x\n😀", "x\n😀!\n");
        assert_eq!(edits[0].to_end_of_file, Some(2));
        // an edit that stops short of the last line has no such end
        let edits = line_edits("x\ncafé", "y\ncafé");
        assert_eq!(edits[0].to_end_of_file, None);
    }

    #[test]
    fn an_insertion_is_an_empty_range() {
        let edits = line_edits("a\nc\n", "a\nb\nc\n");
        assert_eq!(
            edits,
            [LineEdit {
                start_line: 1,
                end_line: 1,
                text: "b\n".into(),
                to_end_of_file: None
            }]
        );
        assert_eq!(first_changed_line("a\nc\n", "a\nb\nc\n"), Some(2));
        assert_eq!(lines_changed(&edits), 1);
    }

    #[test]
    fn a_new_file_is_one_edit_from_empty() {
        let edits = line_edits("", "a\nb\n");
        assert_eq!(
            edits,
            [LineEdit {
                start_line: 0,
                end_line: 0,
                text: "a\nb\n".into(),
                to_end_of_file: None
            }]
        );
        assert_eq!(lines_changed(&edits), 2);
    }

    #[test]
    fn separate_runs_of_changes_are_separate_edits() {
        let edits = line_edits("a\nb\nc\nd\ne\n", "A\nb\nc\nd\nE\n");
        assert_eq!(edits.len(), 2);
        assert_eq!((edits[0].start_line, edits[0].end_line), (0, 1));
        assert_eq!((edits[1].start_line, edits[1].end_line), (4, 5));
        assert_eq!(lines_changed(&edits), 2);
    }

    #[test]
    fn a_deletion_has_no_text() {
        let edits = line_edits("a\nb\nc\n", "a\nc\n");
        assert_eq!(
            edits,
            [LineEdit {
                start_line: 1,
                end_line: 2,
                text: String::new(),
                to_end_of_file: None
            }]
        );
    }
}
