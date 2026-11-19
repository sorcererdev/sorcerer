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
use crate::domain::finding::status_word;
use crate::usecases::explain_finding::{CitedText, explain_finding};

pub(super) fn why(project: &Project, env: &dyn Env, id: &str, out: &mut dyn Write) -> Result<i32> {
    let view = FilesView::new(Arc::new(project.clone()), GrammarCache::user(env));
    let x = explain_finding(
        project,
        &JsonFindingStore::for_project(project)?,
        &ScryToml::for_project(project),
        &view,
        id,
    )?;
    let f = &x.finding;
    let file = f.place.file.display();
    let lines = lines_text(f.place.lines.start, f.place.lines.end);
    writeln!(
        out,
        "# {}: {file}:{lines}\n\n{}\n",
        f.issue_type, f.statement
    )?;
    if let Some((id, text)) = &x.rule {
        match text {
            Some(t) => writeln!(out, "Rule: {id}\n{t}")?,
            None => writeln!(out, "Rule: {id} (not in {STEERING_FILE} now)")?,
        }
        writeln!(out)?;
    }
    match (&f.place.unit, f.place.unit_start) {
        (Some(u), Some(at)) => writeln!(out, "Unit: {u} (starts at {file}:{at})\n")?,
        (Some(u), None) => writeln!(out, "Unit: {u}\n")?,
        _ => {}
    }
    if !x.evidence.is_empty() {
        writeln!(out, "Evidence:")?;
        for (e, text) in &x.evidence {
            writeln!(
                out,
                "{}:{}",
                e.file.display(),
                lines_text(e.lines.start, e.lines.end)
            )?;
            match text {
                CitedText::NotShown => writeln!(out, "(not shown: fenced or outside the project)")?,
                CitedText::Text(text) => writeln!(out, "{}", cited_range(text, e.lines))?,
                CitedText::Unreadable => writeln!(out, "(the file cannot be read)")?,
            }
        }
        writeln!(out)?;
    }
    if let Some(v) = &f.verdict {
        writeln!(out, "Checked: {}: {}", v.verdict, v.why)?;
        if !v.cited.is_empty() {
            writeln!(out, "Cited by the check:")?;
            for c in &v.cited {
                writeln!(out, "{}: {}", c.label(), c.outcome())?;
            }
        }
    }
    if let Some(k) = f.scores.confidence {
        writeln!(out, "Confidence: {k:.2}")?;
    }
    writeln!(
        out,
        "Seen: {} since {}\n",
        if f.seen == 1 {
            "1 time".to_string()
        } else {
            format!("{} times", f.seen)
        },
        f.first.format("%Y-%m-%d")
    )?;
    writeln!(out, "History:")?;
    for (at, s) in &f.history {
        writeln!(
            out,
            "{}  {}",
            at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            status_word(*s)
        )?;
    }
    Ok(0)
}
