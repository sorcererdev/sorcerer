//! `scry why`: one finding in full, as Markdown: what it says, the rule
//! and the unit, the code it cites, what the check said, and its history.

use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use super::format::{cited_range, lines_text};
use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::scry_toml::{STEERING_FILE, ScryToml};
use crate::domain::finding::{Evidence, Finding, status_word};
use crate::usecases::explain_finding::{CitedText, Explanation, explain_finding};

/// Prints the finding with this id (or the start of one, if only one
/// finding has it) in full.
pub(super) fn why(project: &Project, env: &dyn Env, id: &str, out: &mut dyn Write) -> Result<i32> {
    let view = FilesView::new(Arc::new(project.clone()), GrammarCache::user(env));
    let explanation = explain_finding(
        project,
        &JsonFindingStore::for_project(project)?,
        &ScryToml::for_project(project),
        &view,
        id,
    )?;
    write!(out, "{}", explanation_text(&explanation))?;
    Ok(0)
}

// The command's text.

/// The explanation as Markdown, a section at a time.
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
fn explanation_text(explanation: &Explanation) -> String {
    let f = &explanation.finding;
    [
        heading_text(f),
        explanation.rule.as_ref().map_or(String::new(), rule_text),
        unit_text(f),
        evidence_text(&explanation.evidence),
        checked_text(f),
        seen_text(f),
        history_text(f),
    ]
    .concat()
}

/// The heading, `# bug: orders.py:2-3`, then the statement.
fn heading_text(f: &Finding) -> String {
    format!(
        "# {}: {}:{}\n\n{}\n\n",
        f.issue_type,
        f.place.file.display(),
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
                f.place.file.display()
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
            format!("{}:{range}\n{code}\n", e.file.display())
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
fn seen_text(f: &Finding) -> String {
    let times = if f.seen == 1 {
        "1 time".to_string()
    } else {
        format!("{} times", f.seen)
    };
    format!("Seen: {times} since {}\n\n", f.first.format("%Y-%m-%d"))
}

/// Each status the finding has had, with when, one a line.
fn history_text(f: &Finding) -> String {
    let lines: String = f
        .history
        .iter()
        .map(|(at, status)| {
            let at = at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            format!("{at}  {}\n", status_word(*status))
        })
        .collect();
    format!("History:\n{lines}")
}
