//! Changes to a file that is already there, made of whole lines: what an `insert` edit or a fix's
//! `replace` does to the text, and the line diff a plan shows for it.

use similar::{ChangeTag, TextDiff};

/// Lines `from_row` up to (not including) `to_row` (rows from 0) replaced by `new`. An insertion
/// has `from_row == to_row` and `old` empty; a deletion has `new` empty. `old` is the text the
/// lines had, kept so a change can be shown and checked; `new` ends with a line ending unless it
/// is empty.
///
/// Example: putting `import os\n` before row 2 is `{ from_row: 2, to_row: 2, old: "", new:
/// "import os\n" }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChange {
    pub from_row: usize,
    pub to_row: usize,
    pub old: String,
    pub new: String,
}

/// One hunk of a line diff: its `@@ -a,b +c,d @@` header and its lines, each starting with ` `
/// (unchanged), `-` (removed) or `+` (added), without the line ending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub lines: Vec<String>,
}

/// The text with the changes made. The changes may come in any order but may not overlap, and
/// two insertions at one row are refused, since their order would be a guess. They are made from
/// the bottom of the file up, so each change's rows are the rows of the text it was planned
/// against.
///
/// Example: in `a\nb\n`, replacing row 1 with `c\n` and inserting `z\n` at row 0 gives `z\na\nc\n`.
pub fn apply_changes(text: &str, changes: &[TextChange]) -> Result<String, String> {
    let mut lines: Vec<String> = text.split_inclusive('\n').map(String::from).collect();
    let mut ordered: Vec<&TextChange> = changes.iter().collect();
    ordered.sort_by_key(|change| (change.from_row, change.to_row));
    refuse_overlaps(&ordered)?;
    if let Some(last) = lines.last_mut()
        && !last.ends_with('\n')
        && ordered
            .iter()
            .any(|change| change.to_row >= lines_len(text))
    {
        last.push_str(line_ending_of(text));
    }
    for change in ordered.iter().rev() {
        if change.to_row > lines.len() {
            return Err(format!(
                "a change to rows {}..{} is past the end of the file ({} lines)",
                change.from_row,
                change.to_row,
                lines.len()
            ));
        }
        let new_lines: Vec<String> = change.new.split_inclusive('\n').map(String::from).collect();
        lines.splice(change.from_row..change.to_row, new_lines);
    }
    Ok(lines.concat())
}

fn lines_len(text: &str) -> usize {
    text.split_inclusive('\n').count()
}

/// Two changes that touch one row, or two insertions at one row.
fn refuse_overlaps(ordered: &[&TextChange]) -> Result<(), String> {
    for pair in ordered.windows(2) {
        let (first, second) = (pair[0], pair[1]);
        let overlap = second.from_row < first.to_row
            || (first.from_row == first.to_row && second.from_row == first.from_row);
        if overlap {
            return Err(format!(
                "two changes touch row {} of the file; a plan changes each line once",
                second.from_row + 1
            ));
        }
    }
    Ok(())
}

/// The line diff from `old` to `new`, one line of context around each change.
///
/// Example: `a\nb\nc\n` to `a\nB\nc\n` is one hunk, `@@ -1,3 +1,3 @@`, with ` a`, `-b`, `+B`, ` c`.
pub fn hunks(old: &str, new: &str) -> Vec<Hunk> {
    let diff = TextDiff::from_lines(old, new);
    let mut unified = diff.unified_diff();
    unified
        .context_radius(1)
        .iter_hunks()
        .map(|hunk| Hunk {
            header: hunk.header().to_string(),
            lines: hunk
                .iter_changes()
                .map(|change| {
                    let sign = match change.tag() {
                        ChangeTag::Equal => ' ',
                        ChangeTag::Delete => '-',
                        ChangeTag::Insert => '+',
                    };
                    let line = change.value().trim_end_matches(['\n', '\r']);
                    format!("{sign}{line}")
                })
                .collect(),
        })
        .collect()
}

/// How many lines a diff adds and removes, for a plan's `(+3 −1)`.
pub fn added_and_removed(hunks: &[Hunk]) -> (usize, usize) {
    let count = |sign: char| {
        hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.starts_with(sign))
            .count()
    };
    (count('+'), count('-'))
}

/// The text with the line ending of `of_file`: `\r\n` when the file uses it, else as written.
///
/// Example: `a\nb\n` for a file of `x\r\n` is `a\r\nb\r\n`.
pub fn with_line_ending(text: &str, of_file: &str) -> String {
    if line_ending_of(of_file) == "\r\n" {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}

/// The file's line ending, judged by its first line: `\r\n` or `\n`.
fn line_ending_of(text: &str) -> &'static str {
    match text.find('\n') {
        Some(at) if at > 0 && text.as_bytes()[at - 1] == b'\r' => "\r\n",
        _ => "\n",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(from_row: usize, to_row: usize, new: &str) -> TextChange {
        TextChange {
            from_row,
            to_row,
            old: String::new(),
            new: new.into(),
        }
    }

    #[test]
    fn changes_apply_bottom_up_in_any_order() {
        let text = "a\nb\nc\n";
        let got = apply_changes(
            text,
            &[
                change(1, 2, "B\n"),
                change(0, 0, "z\n"),
                change(3, 3, "d\n"),
            ],
        )
        .unwrap();
        assert_eq!(got, "z\na\nB\nc\nd\n");
    }

    #[test]
    fn a_deletion_drops_its_lines() {
        assert_eq!(
            apply_changes("a\nb\nc\n", &[change(1, 2, "")]).unwrap(),
            "a\nc\n"
        );
    }

    #[test]
    fn overlapping_changes_are_refused() {
        let error =
            apply_changes("a\nb\nc\n", &[change(0, 2, "x\n"), change(1, 3, "y\n")]).unwrap_err();
        assert_eq!(
            error,
            "two changes touch row 2 of the file; a plan changes each line once"
        );
        assert!(apply_changes("a\n", &[change(1, 1, "x\n"), change(1, 1, "y\n")]).is_err());
    }

    #[test]
    fn an_insertion_after_a_last_line_without_an_ending_gets_one() {
        assert_eq!(
            apply_changes("a\r\nb", &[change(2, 2, "c\r\n")]).unwrap(),
            "a\r\nb\r\nc\r\n"
        );
    }

    #[test]
    fn crlf_is_kept() {
        assert_eq!(with_line_ending("x\ny\n", "a\r\nb\r\n"), "x\r\ny\r\n");
        assert_eq!(with_line_ending("x\ny\n", "a\nb\n"), "x\ny\n");
        let got = apply_changes(
            "a\r\nb\r\n",
            &[change(1, 1, &with_line_ending("n\n", "a\r\n"))],
        )
        .unwrap();
        assert_eq!(got, "a\r\nn\r\nb\r\n");
    }

    #[test]
    fn hunks_show_one_line_of_context() {
        let found = hunks("a\nb\nc\nd\ne\n", "a\nB\nc\nd\ne\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].header, "@@ -1,3 +1,3 @@");
        assert_eq!(found[0].lines, vec![" a", "-b", "+B", " c"]);
        assert_eq!(added_and_removed(&found), (1, 1));
        let crlf = hunks("a\r\nb\r\n", "a\r\nc\r\n");
        assert_eq!(crlf[0].lines, vec![" a", "-b", "+c"]);
    }
}
