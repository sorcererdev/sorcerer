//! `scry why`: a finding in full, as Markdown.

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

/// The finding ID (or an unambiguous prefix) explained on OUT: its
/// statement, rule, unit, evidence, check, confidence, sightings and history.
pub(super) fn why(project: &Project, env: &dyn Env, id: &str, out: &mut dyn Write) -> Result<i32> {
    let view = FilesView::new(Arc::new(project.clone()), GrammarCache::user(env));
    let x = explain_finding(
        project,
        &JsonFindingStore::for_project(project)?,
        &ScryToml::for_project(project),
        &view,
        id,
    )?;
    write!(out, "{}", explanation_text(&x))?;
    Ok(0)
}

// The command's text.

/// The explanation as Markdown, a section at a time.
fn explanation_text(x: &Explanation) -> String {
    let f = &x.finding;
    [
        heading_text(f),
        x.rule.as_ref().map_or(String::new(), rule_text),
        unit_text(f),
        evidence_text(&x.evidence),
        checked_text(f),
        seen_text(f),
        history_text(f),
    ]
    .concat()
}

/// `# bug: orders.py:2-3` and the statement.
fn heading_text(f: &Finding) -> String {
    format!(
        "# {}: {}:{}\n\n{}\n\n",
        f.issue_type,
        f.place.file.display(),
        lines_text(f.place.lines.start, f.place.lines.end),
        f.statement
    )
}

/// The rule cited, with its text, or a note that the steering no longer has it.
fn rule_text((id, text): &(String, Option<String>)) -> String {
    match text {
        Some(t) => format!("Rule: {id}\n{t}\n\n"),
        None => format!("Rule: {id} (not in {STEERING_FILE} now)\n\n"),
    }
}

fn unit_text(f: &Finding) -> String {
    match (&f.place.unit, f.place.unit_start) {
        (Some(u), Some(at)) => format!("Unit: {u} (starts at {}:{at})\n\n", f.place.file.display()),
        (Some(u), None) => format!("Unit: {u}\n\n"),
        _ => String::new(),
    }
}

/// Each cited range and its lines, or why they are not shown.
fn evidence_text(evidence: &[(Evidence, CitedText)]) -> String {
    if evidence.is_empty() {
        return String::new();
    }
    let mut s = "Evidence:\n".to_string();
    for (e, text) in evidence {
        s.push_str(&format!(
            "{}:{}\n",
            e.file.display(),
            lines_text(e.lines.start, e.lines.end)
        ));
        s.push_str(&match text {
            CitedText::NotShown => "(not shown: fenced or outside the project)\n".to_string(),
            CitedText::Text(text) => format!("{}\n", cited_range(text, e.lines)),
            CitedText::Unreadable => "(the file cannot be read)\n".to_string(),
        });
    }
    s.push('\n');
    s
}

/// The check's verdict and what it cited, then the confidence.
fn checked_text(f: &Finding) -> String {
    let mut s = String::new();
    if let Some(v) = &f.verdict {
        s.push_str(&format!("Checked: {}: {}\n", v.verdict, v.why));
        if !v.cited.is_empty() {
            s.push_str("Cited by the check:\n");
            for c in &v.cited {
                s.push_str(&format!("{}: {}\n", c.label(), c.outcome()));
            }
        }
    }
    if let Some(k) = f.scores.confidence {
        s.push_str(&format!("Confidence: {k:.2}\n"));
    }
    s
}

fn seen_text(f: &Finding) -> String {
    format!(
        "Seen: {} since {}\n\n",
        if f.seen == 1 {
            "1 time".to_string()
        } else {
            format!("{} times", f.seen)
        },
        f.first.format("%Y-%m-%d")
    )
}

fn history_text(f: &Finding) -> String {
    let mut s = "History:\n".to_string();
    for (at, status) in &f.history {
        s.push_str(&format!(
            "{}  {}\n",
            at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            status_word(*status)
        ));
    }
    s
}
