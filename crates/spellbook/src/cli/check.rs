//! `spellbook check [FILE…] [--since REV] [--baseline] [--json]`: the rules run over the project
//! (or the named files, or what changed since a commit), and the baseline of known debt.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Result, anyhow};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::adapters::baseline_toml::{BASELINE_FILE, write_baseline};
use crate::adapters::wiring::Wired;
use crate::domain::baseline::{Baseline, BaselineChange};
use crate::domain::finding::Finding;
use crate::domain::rule::{Rule, Severity};
use crate::usecases::check_files::check_files;
use crate::usecases::check_project::check_project;
use crate::usecases::check_since::check_since;
use crate::usecases::fixes_for::{FixOffer, fixes_for, region_fix_offer};

use super::format::count;

/// What a check looks at.
pub(super) enum CheckScope {
    /// Every file of the project; the report is rewritten whole.
    Project,
    /// Only these files (relative to the root).
    Files(Vec<PathBuf>),
    /// The lines changed since a commit, and every region; the report is left alone.
    Since(String),
}

/// Runs the checks and prints one row for each finding, then whether the conventions hold.
/// The exit code is 0 when they hold (hints, warnings and existing findings do not fail), 1 when
/// a new finding is an error. A rules, regions, baseline or settings file that cannot be read,
/// or a commit git does not know, stops with the sentence and exit 2.
pub(super) fn check(
    wired: &Wired,
    scope: &CheckScope,
    as_json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let checking = wired.checking().map_err(|why| anyhow!(why))?;
    let found = match scope {
        CheckScope::Project => check_project(&checking.deps())?,
        CheckScope::Files(files) => check_files(&checking.deps(), files)?,
        CheckScope::Since(rev) => check_since(&checking.deps(), rev)?,
    };
    let offers: Vec<Vec<FixOffer>> = found
        .iter()
        .map(|finding| offers_for(wired, &checking.rules, finding))
        .collect();
    if as_json {
        let report = CheckJson {
            checked: wired.clock.now(),
            since: match scope {
                CheckScope::Since(rev) => Some(rev),
                _ => None,
            },
            findings: found
                .iter()
                .zip(&offers)
                .map(|(finding, fixes)| FindingWithFixes { finding, fixes })
                .collect(),
        };
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        let paid_off = match scope {
            CheckScope::Project => checking.baseline.change_to(&Baseline::of(&found)).paid_off,
            _ => 0,
        };
        write!(
            out,
            "{}",
            check_text(&found, &offers, checking.rules.is_empty(), paid_off)
        )?;
    }
    Ok(i32::from(found.iter().any(is_blocking)))
}

/// `check --baseline`: checks the whole project and records its rule findings as the baseline,
/// replacing the file. Prints how many, and what changed since the last one. Exit 0.
///
/// Example: `baselined 2 findings in .sorcerer/spellbook/baseline.toml (1 paid off, 0 new)`.
pub(super) fn record_baseline(wired: &Wired, as_json: bool, out: &mut dyn Write) -> Result<i32> {
    let checking = wired.checking().map_err(|why| anyhow!(why))?;
    let found = check_project(&checking.deps())?;
    let recorded = Baseline::of(&found);
    let change = checking.baseline.change_to(&recorded);
    write_baseline(&wired.project, &recorded).map_err(|why| anyhow!(why))?;
    if as_json {
        let report = BaselineJson {
            file: BASELINE_FILE,
            baselined: recorded.len(),
            paid_off: change.paid_off,
            new: change.new,
        };
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        writeln!(
            out,
            "{}",
            baselined_text(&recorded, &checking.baseline, change)
        )?;
    }
    Ok(0)
}

/// An error that the baseline does not record: what fails a check.
fn is_blocking(finding: &Finding) -> bool {
    finding.severity == Severity::Error && !finding.existing
}

/// `baselined 2 findings in .sorcerer/spellbook/baseline.toml`, and `(1 paid off, 0 new)` when
/// there was a baseline before.
fn baselined_text(recorded: &Baseline, before: &Baseline, change: BaselineChange) -> String {
    let line = format!(
        "baselined {} in {BASELINE_FILE}",
        count(recorded.len(), "finding", "findings")
    );
    if before.is_empty() {
        return line;
    }
    format!("{line} ({} paid off, {} new)", change.paid_off, change.new)
}

/// `check --baseline --json`.
#[derive(Serialize)]
struct BaselineJson {
    file: &'static str,
    baselined: usize,
    paid_off: usize,
    new: usize,
}

/// `check --json`: the report's shape, each finding with the fixes it offers.
#[derive(Serialize)]
struct CheckJson<'f> {
    checked: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<&'f String>,
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

/// The rows, the closing line, and the baseline entries no finding uses any more.
///
/// Example: one warning gives `! src/a.x:4  some-rule — its message`, then `conventions hold (1
/// finding, none blocking)`; with two existing errors besides, `conventions hold (3 findings, 2
/// existing, none blocking)`.
fn check_text(
    found: &[Finding],
    offers: &[Vec<FixOffer>],
    no_rules: bool,
    paid_off: usize,
) -> String {
    let mut text: String = found
        .iter()
        .zip(offers)
        .map(|(finding, fixes)| finding_row(finding, fixes))
        .collect();
    text.push_str(&verdict_text(found));
    if paid_off > 0 {
        text.push_str(&format!(
            "{} paid off (spellbook check --baseline records less debt)\n",
            count(paid_off, "baseline entry is", "baseline entries are")
        ));
    }
    if no_rules && !found.iter().any(|finding| finding.region.is_some()) {
        text.push_str("this project has no rules yet: add .sorcerer/spellbook/rules.toml\n");
    }
    text
}

/// `1 blocking finding`, `conventions hold (no findings)` or `conventions hold (3 findings, 2
/// existing, none blocking)`.
fn verdict_text(found: &[Finding]) -> String {
    let blocking = found.iter().filter(|f| is_blocking(f)).count();
    let existing = found.iter().filter(|f| f.existing).count();
    let existing_part = if existing > 0 {
        format!(", {existing} existing")
    } else {
        String::new()
    };
    if blocking > 0 {
        format!(
            "{}{existing_part}\n",
            count(blocking, "blocking finding", "blocking findings")
        )
    } else if found.is_empty() {
        "conventions hold (no findings)\n".to_string()
    } else {
        format!(
            "conventions hold ({}{existing_part}, none blocking)\n",
            count(found.len(), "finding", "findings")
        )
    }
}

/// One finding, ending with its fixes when its rule has any.
///
/// Example: `✗ src/a.py:4  commands-use-ports — Commands talk to ports  (fixes: Use a port · Add a
/// port)`. A finding the baseline records is `·` and ends `(existing)`, whatever its severity.
fn finding_row(finding: &Finding, fixes: &[FixOffer]) -> String {
    let mark = match finding.severity {
        _ if finding.existing => "·",
        Severity::Hint => "·",
        Severity::Warn => "!",
        Severity::Error => "✗",
    };
    let existing = if finding.existing { "  (existing)" } else { "" };
    let offered = if fixes.is_empty() {
        String::new()
    } else {
        let labels: Vec<&str> = fixes.iter().map(|fix| fix.label.as_str()).collect();
        format!("  (fixes: {})", labels.join(" · "))
    };
    format!(
        "{mark} {}:{}  {} — {}{offered}{existing}\n",
        finding.file, finding.line, finding.rule, finding.message
    )
}
