//! What Jev reads besides a region's own text: the lines around it, for a paragraph the section
//! it sits in, and the session's changes. Measured on `corpus/lackey/triage` (2026-11-27):
//! with this context, triage offered 34 of 37 asks against 12 with the text alone, at about
//! 3,750 input tokens ($0.00016) a region.
//!
//! No language is named: the lines around are plain lines, and a section is found by the
//! heading marks in `library/prose.toml`.

use serde::Deserialize;
use similar::TextDiff;
use sorcerer_core::domain::unit::Span;

use crate::domain::region::{Region, RegionPlace};

/// Lines of the file shown above the region, and below it.
pub const LINES_ABOVE: usize = 20;
pub const LINES_BELOW: usize = 10;
/// The most of the session's changes Jev reads, in characters (about 3,000 tokens).
pub const SESSION_CHARS: usize = 12_000;
/// The most of the latest change Jev reads, in characters.
pub const LATEST_CHARS: usize = 4_000;

const SHIPPED_PROSE: &str = include_str!("../../library/prose.toml");

/// The context of one region, as Jev's state holds it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TriageContext {
    /// The lines around the region, the region's own marked `> `.
    pub around: String,
    /// For a paragraph under a heading: which heading, and whether the section holds anything
    /// else. `None` for code, and for a paragraph with no heading above it.
    pub section: Option<String>,
    /// The session's changes (this file's first), then the latest change; empty when there
    /// are none.
    pub session_changes: String,
}

/// One file's change in the session, as `git diff` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// Relative to the root, with `/`.
    pub path: String,
    pub diff: String,
}

/// The session's changes as gathered: the files' diffs in the order to show them (the
/// region's file first) and how many changed files were left out to stay under the budget.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionDiffs {
    pub files: Vec<FileDiff>,
    pub left_out: usize,
}

/// The context of a region in the file's text now.
pub fn context_of(
    region: &Region,
    text: &str,
    session: &SessionDiffs,
    latest: &str,
) -> TriageContext {
    let span = region.found_in(text).unwrap_or(region.lines);
    TriageContext {
        around: around_text(text, span),
        section: (region.place == RegionPlace::Prose)
            .then(|| section_text(text, span))
            .flatten(),
        session_changes: session_changes_text(session, latest),
    }
}

/// Up to 20 lines above the region and 10 below, the region's own lines marked `> ` and the
/// others indented by two spaces, so the region stands out among them.
///
/// Example: for line 2 of `a\nb\nc\n`, the text `  a\n> b\n  c\n`.
pub fn around_text(text: &str, span: Span) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = (
        span.start as usize - 1,
        (span.end as usize).min(lines.len()),
    );
    let from = start.saturating_sub(LINES_ABOVE);
    let to = (end + LINES_BELOW).min(lines.len());
    (from..to)
        .map(|n| {
            let mark = if (start..end).contains(&n) {
                "> "
            } else {
                "  "
            };
            format!("{mark}{}\n", lines[n])
        })
        .collect()
}

/// For a paragraph: the heading above it and whether its section holds anything else, which
/// is what tells `Fill this in` under an empty heading from a note in a full one.
///
/// Example: `TBD` under `## Limits` with the next heading right after it is "`text` is alone
/// under the heading `## Limits`: the section holds nothing else".
pub fn section_text(text: &str, span: Span) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = (
        span.start as usize - 1,
        (span.end as usize).min(lines.len()),
    );
    let heading = (0..start).rev().find(|&n| is_heading(lines[n]))?;
    let section_end = (end..lines.len())
        .find(|&n| is_heading(lines[n]))
        .unwrap_or(lines.len());
    let others = (heading + 1..section_end)
        .filter(|n| !(start..end).contains(n) && !lines[*n].trim().is_empty())
        .count();
    let title = lines[heading].trim();
    Some(match others {
        0 => format!("`text` is alone under the heading `{title}`: the section holds nothing else"),
        n => format!("under the heading `{title}`, whose section holds {n} other lines"),
    })
}

/// The session's diffs in their order, at most 12,000 characters of them (a file's diff that
/// does not fit is counted instead), then the latest change, cut at 4,000.
///
/// Example: diffs of 7,000, 6,000 and 2,000 characters show the first and the third, then
/// `(and 1 more files changed)`.
pub fn session_changes_text(session: &SessionDiffs, latest: &str) -> String {
    let (mut text, mut left_out) = (String::new(), session.left_out);
    for file in &session.files {
        if text.len() + file.diff.len() > SESSION_CHARS {
            left_out += 1;
            continue;
        }
        text.push_str(&file.diff);
    }
    if left_out > 0 {
        text.push_str(&format!("(and {left_out} more files changed)\n"));
    }
    text.push_str(cut_at(latest, LATEST_CHARS));
    text
}

/// The change of the last look, as a diff of the file: `--- a/<file>`, `+++ b/<file>` and
/// its hunks with three lines around each.
pub fn latest_change(file: &str, before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{file}"), &format!("b/{file}"))
        .to_string()
}

/// A new file's text as the diff that adds it, for a file git does not track yet.
pub fn added_file_diff(file: &str, text: &str) -> String {
    TextDiff::from_lines("", text)
        .unified_diff()
        .header("/dev/null", &format!("b/{file}"))
        .to_string()
}

/// The text cut to at most `limit` bytes, at a character boundary.
fn cut_at(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let end = (0..=limit)
        .rev()
        .find(|&n| text.is_char_boundary(n))
        .unwrap_or(0);
    &text[..end]
}

#[derive(Deserialize)]
struct ProseFile {
    heading_marks: Vec<String>,
}

/// The marks a heading line starts with in prose, from `library/prose.toml`.
/// Whether a line of prose is a heading: it starts with one of the marks in
/// `library/prose.toml` (`# Questions`, say).
pub fn is_heading(line: &str) -> bool {
    prose_marks()
        .iter()
        .any(|mark| line.starts_with(mark.as_str()))
}

fn prose_marks() -> Vec<String> {
    toml::from_str::<ProseFile>(SHIPPED_PROSE)
        .map(|file| file.heading_marks)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u32, end: u32) -> Span {
        Span { start, end }
    }

    #[test]
    fn the_lines_around_mark_the_regions_own() {
        assert_eq!(around_text("a\nb\nc\n", span(2, 2)), "  a\n> b\n  c\n");
        let long: String = (1..=50).map(|n| format!("{n}\n")).collect();
        let around = around_text(&long, span(30, 30));
        assert!(around.starts_with("  10\n") && around.ends_with("  40\n"));
    }

    #[test]
    fn a_paragraph_alone_under_its_heading_is_told_from_one_in_a_full_section() {
        let text = "# Title\n\n## Example\n\nFill this in\n\n## Inside\n\nA library.\n";
        assert_eq!(
            section_text(text, span(5, 5)).unwrap(),
            "`text` is alone under the heading `## Example`: the section holds nothing else"
        );
        let full = "## Inside\n\nA library.\n\nflesh out\n";
        assert_eq!(
            section_text(full, span(5, 5)).unwrap(),
            "under the heading `## Inside`, whose section holds 1 other lines"
        );
        assert_eq!(section_text("no heading\n", span(1, 1)), None);
    }

    #[test]
    fn the_session_is_cut_at_its_budget_and_ends_with_the_latest_change() {
        let file = |path: &str, size: usize| FileDiff {
            path: path.into(),
            diff: "x".repeat(size),
        };
        let session = SessionDiffs {
            files: vec![file("a", 7_000), file("b", 6_000), file("c", 2_000)],
            left_out: 1,
        };
        let text = session_changes_text(&session, "LATEST");
        assert_eq!(text.len(), 9_000 + "(and 2 more files changed)\n".len() + 6);
        assert!(text.ends_with("(and 2 more files changed)\nLATEST"));
    }

    #[test]
    fn the_latest_change_and_a_new_file_read_as_diffs() {
        let diff = latest_change("a.md", "x\n", "x\ny\n");
        assert!(diff.starts_with("--- a/a.md\n+++ b/a.md\n"));
        assert!(diff.contains("+y\n"));
        assert!(added_file_diff("n.txt", "hi\n").contains("+++ b/n.txt\n@@ -0,0 +1 @@\n+hi\n"));
    }
}
