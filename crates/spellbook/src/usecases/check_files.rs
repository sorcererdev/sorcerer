//! Checking files: each file's rules run, the allow comments read, the report kept.

use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Error, Result};

use crate::domain::allow::{AllowComment, allow_comments, silencing};
use crate::domain::finding::{
    ALLOW_WITHOUT_REASON, Finding, RawFinding, UNKNOWN_RULE, fingerprint,
};
use crate::domain::report::sort_findings;
use crate::domain::rule::{Rule, Severity};
use crate::ports::clock::Clock;
use crate::ports::grammars::Grammars;
use crate::ports::report_store::ReportStore;
use crate::ports::rule_runner::RuleRunner;

/// What checking reads and writes.
pub struct CheckDeps<'a> {
    pub view: &'a dyn CodebaseView,
    pub grammars: &'a dyn Grammars,
    pub runner: &'a dyn RuleRunner,
    pub report: &'a dyn ReportStore,
    pub clock: &'a dyn Clock,
    pub rules: &'a [Rule],
    /// `[spellbook] exclude`: globs of files no rule looks at.
    pub exclude: &'a [String],
}

/// Checks the given files (relative to the root) and keeps the report's findings for just
/// those files. Returns their findings, by file then line.
///
/// Example: a command with `import sqlite3` on line 4 gives one `commands-use-ports` finding at
/// line 4; with `# spellbook:allow commands-use-ports — migration` above it, none.
pub fn check_files(deps: &CheckDeps, files: &[PathBuf]) -> Result<Vec<Finding>> {
    let found = findings_of_files(deps, files)?;
    let names: Vec<String> = files.iter().map(|file| forward_slashes(file)).collect();
    deps.report
        .replace_files(&names, found.clone(), deps.clock.now())?;
    Ok(found)
}

/// Every finding in the given files, nothing saved.
pub(super) fn findings_of_files(deps: &CheckDeps, files: &[PathBuf]) -> Result<Vec<Finding>> {
    let excluded = excluded_files(deps.exclude)?;
    let mut found = Vec::new();
    for file in files {
        found.extend(findings_of_file(deps, &excluded, file)?);
    }
    sort_findings(&mut found);
    Ok(found)
}

fn excluded_files(globs: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in globs {
        let glob = Glob::new(pattern).map_err(|e| Error::Config {
            file: PathBuf::from(".sorcerer/config.toml"),
            why: format!("[spellbook] exclude: `{pattern}` is not a glob ({e})"),
        })?;
        builder.add(glob);
    }
    builder.build().map_err(|e| Error::Config {
        file: PathBuf::from(".sorcerer/config.toml"),
        why: format!("[spellbook] exclude: {e}"),
    })
}

/// One file: what its rules find, less what an allow comment silences, plus the warnings on
/// the allow comments themselves.
fn findings_of_file(deps: &CheckDeps, excluded: &GlobSet, file: &Path) -> Result<Vec<Finding>> {
    let name = forward_slashes(file);
    if excluded.is_match(&name) {
        return Ok(Vec::new());
    }
    let Some(loaded) = deps.grammars.for_path(file)? else {
        return Ok(Vec::new());
    };
    let Ok(text) = deps.view.text(file) else {
        return Ok(Vec::new());
    };
    let rules_here: Vec<&Rule> = deps.rules.iter().filter(|r| r.applies_to(&name)).collect();
    let raw = if rules_here.is_empty() {
        Vec::new()
    } else {
        one_per_line(deps.runner.findings_in(file, &text, &rules_here)?)
    };
    let allows = match &loaded.language.comment {
        Some(marker) => allow_comments(&text, marker),
        None => Vec::new(),
    };
    let mut found = unsilenced(deps.rules, &name, raw, &allows);
    found.extend(allow_warnings(deps.rules, &name, &allows));
    Ok(found)
}

/// One finding a line for each rule.
fn one_per_line(mut raw: Vec<RawFinding>) -> Vec<RawFinding> {
    raw.sort_by(|a, b| (&a.rule, a.line).cmp(&(&b.rule, b.line)));
    raw.dedup_by(|a, b| a.rule == b.rule && a.line == b.line);
    raw
}

/// The findings no allow comment names, as findings of the file.
fn unsilenced(
    rules: &[Rule],
    file: &str,
    raw: Vec<RawFinding>,
    allows: &[AllowComment],
) -> Vec<Finding> {
    raw.into_iter()
        .filter(|finding| silencing(allows, &finding.rule, finding.line).is_none())
        .filter_map(|finding| {
            let rule = rules.iter().find(|rule| rule.id == finding.rule)?;
            Some(Finding {
                fingerprint: fingerprint(&finding.rule, &finding.node_text),
                rule: finding.rule,
                file: file.to_string(),
                line: finding.line,
                severity: rule.severity,
                message: rule.message.clone(),
            })
        })
        .collect()
}

/// A warning on each allow comment that names no rule, and on each that gives no reason.
fn allow_warnings(rules: &[Rule], file: &str, allows: &[AllowComment]) -> Vec<Finding> {
    allows
        .iter()
        .filter_map(|allow| {
            let known = rules.iter().any(|rule| rule.id == allow.rule);
            let (rule, message) = if !known {
                (UNKNOWN_RULE, unknown_rule_text(rules, &allow.rule))
            } else if !allow.has_reason {
                (ALLOW_WITHOUT_REASON, no_reason_text(&allow.rule))
            } else {
                return None;
            };
            Some(Finding {
                fingerprint: fingerprint(rule, &allow.rule),
                rule: rule.to_string(),
                file: file.to_string(),
                line: allow.line,
                severity: Severity::Warn,
                message,
            })
        })
        .collect()
}

fn unknown_rule_text(rules: &[Rule], named: &str) -> String {
    let ids: Vec<&str> = rules.iter().map(|rule| rule.id.as_str()).collect();
    if ids.is_empty() {
        return format!("`spellbook:allow {named}` names no rule (this project has no rules)");
    }
    format!(
        "`spellbook:allow {named}` names no rule (the rules are: {})",
        ids.join(", ")
    )
}

fn no_reason_text(rule: &str) -> String {
    format!(
        "`spellbook:allow {rule}` gives no reason: write `spellbook:allow {rule} — why it is fine here`"
    )
}
