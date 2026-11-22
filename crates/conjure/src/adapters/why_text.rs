//! The text of *Show why* for a conjured change: what it did, then the situation the writer
//! read, its reply as it came, and the facts about the unit before the change.

use crate::adapters::lsp::changes::{LandedChange, LandedFile};
use crate::domain::record::RecordedFacts;

/// The change in full, as `.sorcerer/state/conjure/why-<id>.md` holds it.
///
/// Example:
///
/// ```text
/// # fill total
///
/// haiku · $0.001 · 4 s · change 3fa9c0
///
/// - src/a.py: 1 line changed, parses
/// - src/a__test.py (new): 5 lines changed, parses
///
/// ## The situation
/// …
/// ```
pub(crate) fn why_text(change: &LandedChange) -> String {
    format!(
        "# {}\n\n{} · ${:.3} · {:.0} s · change {}\n\n{}{}\n## The situation\n\n{}\n\n## The reply\n\n{}\n\n## The facts\n\n{}",
        change.goal_text(),
        change.model,
        change.cost.unwrap_or(0.0),
        change.seconds,
        change.id,
        files_text(change),
        not_written_text(change),
        change.situation.trim_end(),
        change.reply.trim_end(),
        facts_text(&change.record.facts),
    )
}

fn files_text(change: &LandedChange) -> String {
    if change.files.is_empty() {
        return "- no file changed\n".into();
    }
    change.files.iter().map(file_line).collect()
}

/// `- src/a.py: 3 lines changed, parses`.
fn file_line(file: &LandedFile) -> String {
    let new = if file.new { " (new)" } else { "" };
    let lines = if file.lines_changed == 1 {
        "1 line".to_string()
    } else {
        format!("{} lines", file.lines_changed)
    };
    let parses = match (file.parsed, file.parse_errors.first()) {
        (false, _) => "not parsed".to_string(),
        (true, None) => "parses".to_string(),
        (true, Some(line)) => format!("does not parse (line {line})"),
    };
    format!(
        "- {}{new}: {lines} changed, {parses}\n",
        file.relative_text()
    )
}

fn not_written_text(change: &LandedChange) -> String {
    if change.record.dropped.is_empty() {
        return String::new();
    }
    format!("- not written: {}\n", change.record.dropped.join(", "))
}

/// The facts about the unit before the change, one a line.
fn facts_text(facts: &RecordedFacts) -> String {
    let lines = |lines: &[u32]| {
        if lines.is_empty() {
            "none".to_string()
        } else {
            let lines: Vec<String> = lines.iter().map(u32::to_string).collect();
            format!("lines {}", lines.join(", "))
        }
    };
    let empty = if facts.body_empty { "yes" } else { "no" };
    format!(
        "- does not parse: {}\n- TODO comments: {}\n- empty body: {empty}\n",
        lines(&facts.parse_errors),
        lines(&facts.todo_lines),
    )
}
