//! `scry list`.

use std::io::Write;

use anyhow::Result;
use serde_json::json;
use sorcerer_core::domain::project::Project;

use super::format::{cut_to, table};
use crate::adapters::json_finding_store::JsonFindingStore;
use crate::domain::finding::status_word;
use crate::usecases::list_findings::{ListRequest, list_findings};

/// The findings of STATUSES (all when none), open first, then file and line.
pub(super) fn list(
    project: &Project,
    statuses: &[String],
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let fs = list_findings(
        &JsonFindingStore::for_project(project)?,
        &ListRequest {
            statuses: statuses.to_vec(),
        },
    )?;
    if json {
        let v: Vec<_> = fs
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
        writeln!(out, "{}", json!({ "findings": v }))?;
    } else {
        let rows: Vec<Vec<String>> = fs
            .iter()
            .map(|f| {
                vec![
                    status_word(f.status),
                    f.issue_type.clone(),
                    f.place.file.display().to_string(),
                    f.place.lines.start.to_string(),
                    f.rule.clone().unwrap_or_else(|| "-".into()),
                    cut_to(80, &f.statement),
                ]
            })
            .collect();
        write!(
            out,
            "{}",
            table(
                &["status", "issue type", "file", "line", "rule", "statement"],
                &rows
            )
        )?;
    }
    Ok(0)
}
