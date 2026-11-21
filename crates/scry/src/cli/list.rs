//! `scry list`: the stored findings, as a table or as JSON.

use std::io::Write;

use anyhow::Result;
use serde_json::json;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;

use super::format::{Output, cut_to, table};
use crate::adapters::json_finding_store::JsonFindingStore;
use crate::domain::finding::{Finding, status_word};
use crate::usecases::list_findings::{ListRequest, list_findings};

/// Prints the stored findings with the given statuses (every finding when
/// none are given), open ones first, then by file and line.
pub(super) fn list(
    project: &Project,
    statuses: &[String],
    output: Output,
    out: &mut dyn Write,
) -> Result<i32> {
    let findings = list_findings(
        &JsonFindingStore::for_project(project)?,
        &ListRequest {
            statuses: statuses.to_vec(),
        },
    )?;
    let text = match output {
        Output::Json => findings_json_text(&findings),
        Output::Text => findings_table_text(&findings),
    };
    write!(out, "{text}")?;
    Ok(0)
}

// The command's text.

/// One JSON object holding the findings.
fn findings_json_text(findings: &[Finding]) -> String {
    let v: Vec<_> = findings
        .iter()
        .map(|f| {
            json!({
                "id": f.id.0,
                "status": status_word(f.status),
                "issue_type": f.issue_type,
                "file": f.place.file,
                "line": f.place.lines.start,
                "rule": f.rule,
                "statement": f.statement,
                "confidence": f.scores.confidence,
                "verdict": f.verdict.as_ref().map(|v| v.verdict.clone()),
                "seen": f.seen,
            })
        })
        .collect();
    format!("{}\n", json!({ "findings": v }))
}

/// A table with a row for each finding, its statement cut to 80
/// characters.
fn findings_table_text(findings: &[Finding]) -> String {
    let rows: Vec<Vec<String>> = findings
        .iter()
        .map(|f| {
            vec![
                status_word(f.status),
                f.issue_type.clone(),
                forward_slashes(&f.place.file),
                f.place.lines.start.to_string(),
                f.rule.clone().unwrap_or_else(|| "-".into()),
                cut_to(80, &f.statement),
            ]
        })
        .collect();
    table(
        &["status", "issue type", "file", "line", "rule", "statement"],
        &rows,
    )
}
