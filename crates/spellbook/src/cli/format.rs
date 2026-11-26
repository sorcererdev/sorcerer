//! How the commands print rows.

use crate::domain::plan::{FileStatus, Plan, PlannedFile};

/// Rows as columns, each column as wide as its widest cell, two spaces apart, each line
/// indented by `indent` and with no space at its end.
///
/// Example: `columns("  ", &[vec!["field", "a.x:10"], vec!["handle", "a.x:30"]])` is
/// `"  field   a.x:10\n  handle  a.x:30\n"`.
pub(super) fn columns(indent: &str, rows: &[Vec<String>]) -> String {
    let count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..count)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    rows.iter()
        .map(|row| {
            let padded: Vec<String> = row
                .iter()
                .zip(&widths)
                .map(|(cell, width)| format!("{cell:<width$}"))
                .collect();
            format!("{indent}{}\n", padded.join("  ").trim_end())
        })
        .collect()
}

/// `1 hole`, `3 holes`.
pub(super) fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A planned file's row: `+` new with its length, `~` changed with its counts (and the region
/// that rewrote it), `=` left as it is.
///
/// Example: `["~ src/a/mod.x", "(region modules: +1 −0)"]`, or for a region `sync --init` set
/// up, `["~ src/a/mod.x", "(region modules: set up, 3 items)"]`.
pub(super) fn file_row(file: &PlannedFile) -> Vec<String> {
    match file.status {
        FileStatus::New => vec![
            format!("+ {}", file.path),
            format!("({} lines)", file.lines),
        ],
        FileStatus::Changed => vec![format!("~ {}", file.path), changed_text(file)],
        FileStatus::Skipped => vec![format!("= {}", file.path), "(exists, skipped)".into()],
        FileStatus::Unchanged => vec![format!("= {}", file.path), "(unchanged)".into()],
    }
}

fn changed_text(file: &PlannedFile) -> String {
    let counts = format!("+{} \u{2212}{}", file.added, file.removed);
    match (&file.region, file.set_up) {
        (None, _) => format!("({counts})"),
        (Some(region), None) => format!("(region {region}: {counts})"),
        (Some(region), Some(items)) => {
            format!(
                "(region {region}: set up, {})",
                count(items as usize, "item", "items")
            )
        }
    }
}

/// A changed file's diff: its path, then each hunk's header and lines.
pub(super) fn hunks_text(file: &PlannedFile) -> String {
    let mut text = format!("--- {}\n", file.path);
    for hunk in &file.hunks {
        text.push_str(&hunk.header);
        text.push('\n');
        for line in &hunk.lines {
            text.push_str(line);
            text.push('\n');
        }
    }
    text
}

/// The plan's notes, one line each (a region not set up, regions not refreshed).
pub(super) fn notes_text(plan: &Plan) -> String {
    plan.notes.iter().map(|note| format!("{note}\n")).collect()
}
