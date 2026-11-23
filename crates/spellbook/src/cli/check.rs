//! `spellbook check [FILE…] [--json]`: the rules run over the project (or the named files).

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Result, anyhow};

use crate::adapters::wiring::Wired;
use crate::domain::finding::Finding;
use crate::domain::report::ReportFile;
use crate::domain::rule::Severity;
use crate::usecases::check_files::check_files;
use crate::usecases::check_project::check_project;

use super::format::count;

/// Runs the checks and prints one row for each finding, then whether the conventions hold.
/// The exit code is 0 when they hold (hints and warnings do not fail), 1 when a finding is an
/// error. A rules file that cannot be read stops with the sentence and exit 2.
pub(super) fn check(
    wired: &Wired,
    files: &[PathBuf],
    as_json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let checking = wired.checking().map_err(|why| anyhow!(why))?;
    let found = if files.is_empty() {
        check_project(&checking.deps())?
    } else {
        check_files(&checking.deps(), files)?
    };
    if as_json {
        let report = ReportFile {
            checked: Some(wired.clock.now()),
            findings: found.clone(),
        };
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        write!(out, "{}", check_text(&found, checking.rules.is_empty()))?;
    }
    Ok(i32::from(
        found.iter().any(|f| f.severity == Severity::Error),
    ))
}

/// The rows and the closing line.
///
/// Example: one warning gives `! src/a.x:4  some-rule — its message`, then `conventions hold (1
/// finding, none blocking)`.
fn check_text(found: &[Finding], no_rules: bool) -> String {
    let mut text: String = found.iter().map(finding_row).collect();
    let blocking = found
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    if blocking > 0 {
        text.push_str(&format!(
            "{}\n",
            count(blocking, "blocking finding", "blocking findings")
        ));
    } else if found.is_empty() {
        text.push_str("conventions hold (no findings)\n");
    } else {
        text.push_str(&format!(
            "conventions hold ({}, none blocking)\n",
            count(found.len(), "finding", "findings")
        ));
    }
    if no_rules {
        text.push_str("this project has no rules yet: add .sorcerer/spellbook/rules.toml\n");
    }
    text
}

fn finding_row(finding: &Finding) -> String {
    let mark = match finding.severity {
        Severity::Hint => "·",
        Severity::Warn => "!",
        Severity::Error => "✗",
    };
    format!(
        "{mark} {}:{}  {} — {}\n",
        finding.file, finding.line, finding.rule, finding.message
    )
}
