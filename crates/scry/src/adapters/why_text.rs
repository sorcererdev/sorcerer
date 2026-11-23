//! A finding in full, as Markdown: what it says, the rule and the unit, the
//! code it cites, what the check said, and its history. `scry why` prints
//! it; the editor's *Show why* writes it to a file and opens it.

use chrono::FixedOffset;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::Span;

use crate::adapters::scry_toml::STEERING_FILE;
use crate::domain::finding::{Evidence, Finding, status_word};
use crate::usecases::explain_finding::{CitedText, Explanation};

/// A finding in full as Markdown, a section at a time: what `scry why` prints and the
/// editor's *Show why* opens. Times are in the user's time zone (`offset`).
///
/// Example (shortened):
/// ```text
/// # bug: orders.py:41
///
/// cancel() returns None when the order was already shipped.
///
/// Unit: Order.cancel (starts at orders.py:30)
///
/// Evidence:
/// orders.py:41
///   39     if self.shipped:
///   40         log.info("shipped")
///   41         return
/// ...
/// ```
pub fn why_text(explanation: &Explanation, offset: FixedOffset) -> String {
    let f = &explanation.finding;
    [
        heading_text(f),
        explanation.rule.as_ref().map_or(String::new(), rule_text),
        unit_text(f),
        evidence_text(&explanation.evidence),
        checked_text(f),
        seen_text(f, offset),
        history_text(f, offset),
    ]
    .concat()
}

/// The heading, `# bug: orders.py:2-3`, then the statement.
fn heading_text(f: &Finding) -> String {
    format!(
        "# {}: {}:{}\n\n{}\n\n",
        f.issue_type,
        forward_slashes(&f.place.file),
        lines_text(f.place.lines.start, f.place.lines.end),
        f.statement
    )
}

/// The rule cited, with its text, or a note that the steering no longer
/// has it.
fn rule_text((id, text): &(String, Option<String>)) -> String {
    match text {
        Some(text) => format!("Rule: {id}\n{text}\n\n"),
        None => format!("Rule: {id} (not in {STEERING_FILE} now)\n\n"),
    }
}

/// The unit the finding is in, and where it starts, when it is in one.
fn unit_text(f: &Finding) -> String {
    match (&f.place.unit, f.place.unit_start) {
        (Some(unit), Some(start)) => {
            format!(
                "Unit: {unit} (starts at {}:{start})\n\n",
                forward_slashes(&f.place.file)
            )
        }
        (Some(unit), None) => format!("Unit: {unit}\n\n"),
        _ => String::new(),
    }
}

/// Each cited range with its code around it, or why the code is not
/// shown. Nothing when the finding cites no evidence.
fn evidence_text(evidence: &[(Evidence, CitedText)]) -> String {
    if evidence.is_empty() {
        return String::new();
    }
    let cited: String = evidence
        .iter()
        .map(|(e, text)| {
            let range = lines_text(e.lines.start, e.lines.end);
            let code = match text {
                CitedText::NotShown => "(not shown: fenced or outside the project)".to_string(),
                CitedText::Text(text) => cited_range(text, e.lines),
                CitedText::Unreadable => "(the file cannot be read)".to_string(),
            };
            format!("{}:{range}\n{code}\n", forward_slashes(&e.file))
        })
        .collect();
    format!("Evidence:\n{cited}\n")
}

/// What the claim check said and what it cited (when it ran), then the
/// confidence (when there is one).
fn checked_text(f: &Finding) -> String {
    let check = f.verdict.as_ref().map(|verdict| {
        let cited: String = verdict
            .cited
            .iter()
            .map(|c| format!("{}: {}\n", c.label(), c.outcome()))
            .collect();
        let cited = if cited.is_empty() {
            cited
        } else {
            format!("Cited by the check:\n{cited}")
        };
        format!("Checked: {}: {}\n{cited}", verdict.verdict, verdict.why)
    });
    let confidence = f
        .scores
        .confidence
        .map(|confidence| format!("Confidence: {confidence:.2}\n"));
    format!(
        "{}{}",
        check.unwrap_or_default(),
        confidence.unwrap_or_default()
    )
}

/// How often passes have raised it, and since when: `Seen: 3 times since
/// 2026-11-18`.
fn seen_text(f: &Finding, offset: FixedOffset) -> String {
    let times = if f.seen == 1 {
        "1 time".to_string()
    } else {
        format!("{} times", f.seen)
    };
    format!(
        "Seen: {times} since {}\n\n",
        f.first.with_timezone(&offset).format("%Y-%m-%d")
    )
}

/// Each status the finding has had, with when, one a line.
fn history_text(f: &Finding, offset: FixedOffset) -> String {
    let lines: String = f
        .history
        .iter()
        .map(|(at, status)| {
            let at = at
                .with_timezone(&offset)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            format!("{at}  {}\n", status_word(*status))
        })
        .collect();
    format!("History:\n{lines}")
}

/// The cited lines, with 2 lines either side, each numbered the way the
/// claim prompt numbers them. A range past the end of the file stops at
/// its last line.
///
/// Example: lines 5-6 of a long file show lines 3 to 8, as `   3 code`.
fn cited_range(text: &str, lines: Span) -> String {
    let file_lines: Vec<&str> = text.lines().collect();
    let first = lines.start.saturating_sub(2).max(1) as usize;
    let last = (lines.end.max(lines.start) as usize + 2).min(file_lines.len());
    (first..=last)
        .filter_map(|n| file_lines.get(n - 1).map(|code| format!("{n:>4} {code}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A line range in short: `2` for one line, `2-5` for several.
pub fn lines_text(first: u32, last: u32) -> String {
    if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    }
}
