//! `spellbook check [FILE…] [--json]`: the rules run over the project (or the named files).

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Result, anyhow};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::adapters::wiring::Wired;
use crate::domain::finding::Finding;
use crate::domain::rule::{Rule, Severity};
use crate::usecases::check_files::check_files;
use crate::usecases::check_project::check_project;
use crate::usecases::fixes_for::{FixOffer, fixes_for, region_fix_offer};

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
    let offers: Vec<Vec<FixOffer>> = found
        .iter()
        .map(|finding| offers_for(wired, &checking.rules, finding))
        .collect();
    if as_json {
        let report = CheckJson {
            checked: wired.clock.now(),
            findings: found
                .iter()
                .zip(&offers)
                .map(|(finding, fixes)| FindingWithFixes { finding, fixes })
                .collect(),
        };
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        write!(
            out,
            "{}",
            check_text(&found, &offers, checking.rules.is_empty())
        )?;
    }
    Ok(i32::from(
        found.iter().any(|f| f.severity == Severity::Error),
    ))
}

/// `check --json`: the report's shape, each finding with the fixes it offers.
#[derive(Serialize)]
struct CheckJson<'f> {
    checked: DateTime<Utc>,
    findings: Vec<FindingWithFixes<'f>>,
}

#[derive(Serialize)]
struct FindingWithFixes<'f> {
    #[serde(flatten)]
    finding: &'f Finding,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fixes: &'f Vec<FixOffer>,
}

/// The fixes of the finding's rule, filled from the finding; a region's finding offers `sync`.
fn offers_for(wired: &Wired, rules: &[Rule], finding: &Finding) -> Vec<FixOffer> {
    if finding.region.is_some() {
        return region_fix_offer(finding).into_iter().collect();
    }
    match rules.iter().find(|rule| rule.id == finding.rule) {
        Some(rule) if !rule.fixes.is_empty() => fixes_for(
            &wired.view,
            &wired.generators,
            rule,
            &finding.file,
            &finding.node_text,
        ),
        _ => Vec::new(),
    }
}

/// The rows and the closing line.
///
/// Example: one warning gives `! src/a.x:4  some-rule — its message`, then `conventions hold (1
/// finding, none blocking)`.
fn check_text(found: &[Finding], offers: &[Vec<FixOffer>], no_rules: bool) -> String {
    let mut text: String = found
        .iter()
        .zip(offers)
        .map(|(finding, fixes)| finding_row(finding, fixes))
        .collect();
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
    if no_rules && !found.iter().any(|finding| finding.region.is_some()) {
        text.push_str("this project has no rules yet: add .sorcerer/spellbook/rules.toml\n");
    }
    text
}

/// One finding, ending with its fixes when its rule has any.
///
/// Example: `✗ src/a.py:4  commands-use-ports — Commands talk to ports  (fixes: Use a port · Add a
/// port)`.
fn finding_row(finding: &Finding, fixes: &[FixOffer]) -> String {
    let mark = match finding.severity {
        Severity::Hint => "·",
        Severity::Warn => "!",
        Severity::Error => "✗",
    };
    let offered = if fixes.is_empty() {
        String::new()
    } else {
        let labels: Vec<&str> = fixes.iter().map(|fix| fix.label.as_str()).collect();
        format!("  (fixes: {})", labels.join(" · "))
    };
    format!(
        "{mark} {}:{}  {} — {}{offered}\n",
        finding.file, finding.line, finding.rule, finding.message
    )
}
