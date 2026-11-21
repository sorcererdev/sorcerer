//! How the commands print: findings as text or JSON, and tables.

use std::path::Path;

use serde_json::json;
use sorcerer_core::domain::paths::forward_slashes;

use crate::adapters::why_text::lines_text;
use crate::domain::finding::Finding;

/// How a command prints what it found: as text for a person, or as JSON for
/// a program (`--json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Output {
    Text,
    Json,
}

impl Output {
    /// JSON when the `--json` flag is given, else text.
    pub(super) fn json_if(json_flag: bool) -> Output {
        if json_flag {
            Output::Json
        } else {
            Output::Text
        }
    }
}

/// A table: each column as wide as its widest cell, columns two spaces
/// apart, no space at the end of a line.
///
/// Example:
/// ```text
/// status  issue type  file
/// open    bug         orders.py
/// ```
pub(super) fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..header.len())
        .map(|column| {
            let cells = rows.iter().filter_map(|row| row.get(column));
            let widest_cell = cells.map(|cell| cell.chars().count()).max().unwrap_or(0);
            header[column].chars().count().max(widest_cell)
        })
        .collect();
    let line = |cells: Vec<&str>| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        format!("{}\n", padded.join("  ").trim_end())
    };
    let header_line = line(header.to_vec());
    let row_lines = rows
        .iter()
        .map(|row| line(row.iter().map(String::as_str).collect()));
    std::iter::once(header_line).chain(row_lines).collect()
}

/// The text, cut to at most `n` characters, ending in an ellipsis when it
/// was longer: `cut_to(8, "cancel() returns None")` is `cancel(…`.
pub(super) fn cut_to(n: usize, text: &str) -> String {
    if text.chars().count() > n {
        let kept: String = text.chars().take(n - 1).collect();
        format!("{kept}…")
    } else {
        text.to_string()
    }
}

/// A finding as `scry` prints it: where it is, its issue type, its
/// statement wrapped to the terminal's width with the confidence after it,
/// then its evidence and how to read it in full.
///
/// Example:
/// ```text
/// orders.py:41  bug  cancel() returns None when the order was already
///                    shipped, so the caller's refund is skipped.  (0.92)
///   evidence: orders.py:41, orders.py:57-58
///   scry why 3fa9c0
/// ```
pub(super) fn finding_text(f: &Finding, width: usize) -> String {
    let head = format!(
        "{}:{}  {}  ",
        forward_slashes(&f.place.file),
        f.place.lines.start,
        f.issue_type
    );
    let indent = head.chars().count();
    let statement_width = width.saturating_sub(indent).max(30);
    let confidence = format!("({:.2})", f.scores.confidence.unwrap_or(0.0));
    let statement = wrapped_with_tag(&f.statement, &confidence, statement_width)
        .join(&format!("\n{}", " ".repeat(indent)));
    let evidence = if f.evidence.is_empty() {
        String::new()
    } else {
        evidence_line(f)
    };
    format!("{head}{statement}\n{evidence}  scry why {}\n", f.id.0)
}

/// The statement's words in lines of at most `width` characters, with the
/// tag two spaces after the last word, or on a line of its own when it
/// does not fit there.
fn wrapped_with_tag(statement: &str, tag: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![String::new()];
    for word in statement.split_whitespace() {
        let last = lines.last_mut().expect("one line at least");
        if last.is_empty() {
            last.push_str(word);
        } else if last.chars().count() + 1 + word.chars().count() <= width {
            last.push(' ');
            last.push_str(word);
        } else {
            lines.push(word.to_string());
        }
    }
    let last = lines.last_mut().expect("one line at least");
    if last.chars().count() + 2 + tag.len() <= width {
        last.push_str("  ");
        last.push_str(tag);
    } else {
        lines.push(tag.to_string());
    }
    lines
}

/// The finding's evidence on one line: `  evidence: orders.py:2, orders.py:5-6`.
fn evidence_line(f: &Finding) -> String {
    let cited: Vec<String> = f
        .evidence
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                forward_slashes(&e.file),
                lines_text(e.lines.start, e.lines.end)
            )
        })
        .collect();
    format!("  evidence: {}\n", cited.join(", "))
}

/// A finding as one JSON object, as `scry --json` prints it.
pub(super) fn finding_json(f: &Finding) -> serde_json::Value {
    json!({
        "id": f.id.0,
        "issue_type": f.issue_type,
        "rule": f.rule,
        "statement": f.statement,
        "file": f.place.file,
        "lines": [f.place.lines.start, f.place.lines.end],
        "unit": f.place.unit,
        "evidence": f.evidence.iter().map(|e| json!({
            "file": e.file,
            "lines": [e.lines.start, e.lines.end],
        })).collect::<Vec<_>>(),
        "confidence": f.scores.confidence,
        "verdict": f.verdict.as_ref().map(|v| v.verdict.clone()),
        "status": f.status,
    })
}

/// A file under the project's root as its project-relative path, written with `/`
/// (`.sorcerer/scry.toml`); a file elsewhere as its absolute path, in the platform's own form.
pub(super) fn relative_or_absolute(file: &Path, root: &Path) -> String {
    match file.strip_prefix(root) {
        Ok(relative) => forward_slashes(relative),
        Err(_) => file.display().to_string(),
    }
}
