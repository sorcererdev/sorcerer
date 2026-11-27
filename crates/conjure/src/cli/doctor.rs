//! `conjure doctor`: whether conjure can run in a project. It prints a row for each thing
//! conjure needs (git, a model provider, its settings, the languages, scry's findings, the
//! checker of spellbook's rules, the ledger and the records), each `ok` or `FAIL` with what it found, and last whether the
//! editor's server needs the project trusted.
//!
//! It tries the real adapters (the provider, the grammars), so it lives with the wiring's
//! other callers, not among the use cases. It only reads.

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::adapters::providers::default_provider_for;

use crate::adapters::jsonl_change_store::JsonlChangeStore;
use crate::adapters::scry_files::read_findings;
use crate::adapters::spellbook_checker::SpellbookChecker;
use crate::domain::config::{ConjureConfig, PRODUCT, conjure_config};
use crate::domain::language_tables::{imports_table, test_table};
use crate::domain::log_text::count;
use crate::domain::scry_notes::FindingsRead;
use crate::ports::change_store::ChangeStore;
use crate::ports::rule_checker::RuleChecker;

/// Prints the doctor's rows. The exit code is 2 when any row failed, else 0.
pub(super) fn doctor(
    project: &Project,
    env: &dyn Env,
    cache: &GrammarCache,
    out: &mut dyn Write,
) -> Result<i32> {
    let rows = doctor_rows(project, env, cache);
    for row in &rows {
        write!(out, "{}", row_text(row))?;
    }
    let any_failed = rows.iter().any(|row| row.result.is_err());
    Ok(if any_failed { 2 } else { 0 })
}

/// One row of `conjure doctor`: what was checked, and what was found or why it fails.
#[derive(Debug, Clone, PartialEq)]
pub struct DoctorRow {
    pub name: &'static str,
    pub result: std::result::Result<String, String>,
}

/// Whether conjure can run in the project, a row for each thing it needs, in this order: `git`,
/// `provider`, `config`, `languages`, `scry findings`, `spellbook rules`, `ledger`, `records`,
/// `trust`. Changes not checked against spellbook's rules is not a failure: conjure works
/// without spellbook.
pub fn doctor_rows(project: &Project, env: &dyn Env, cache: &GrammarCache) -> Vec<DoctorRow> {
    let row = |name, result| DoctorRow { name, result };
    let config = conjure_config(project);
    vec![
        row("git", git_row(env)),
        row("provider", provider_row(project, env, &config)),
        row("config", config_row(&config)),
        row("languages", languages_row(project, cache)),
        row("scry findings", Ok(scry_findings_row(project))),
        row(
            "spellbook rules",
            Ok(SpellbookChecker::for_project(project, env).describe()),
        ),
        row("ledger", ledger_row(project)),
        row("records", records_row(project)),
        row("trust", trust_row(project, TrustList::user(env).as_ref())),
    ]
}

/// Where `git` was found: conjure reads your uncommitted changes through it.
fn git_row(env: &dyn Env) -> Result<String, String> {
    env.which("git")
        .map(|path| path.display().to_string())
        .ok_or_else(|| {
            "git is not on PATH; conjure reads your uncommitted changes through git. Install it: \
             https://git-scm.com/downloads"
                .to_string()
        })
}

/// The provider a change would use and why, then each alias it resolves to:
/// `claude (claude is on PATH); haiku → haiku, sonnet → sonnet`, or `, try harder off`.
fn provider_row(
    project: &Project,
    env: &dyn Env,
    config: &sorcerer_core::Result<ConjureConfig>,
) -> Result<String, String> {
    let (provider, choice) = default_provider_for(project, env).map_err(|e| e.to_string())?;
    let config = config.as_ref().ok().cloned().unwrap_or_default();
    let resolve = |alias: &str| {
        provider
            .resolve(alias)
            .map(|id| format!("{alias} → {}", id.0))
            .map_err(|e| e.to_string())
    };
    let writer = resolve(&config.writer)?;
    let try_harder = if config.offers_try_harder() {
        resolve(&config.try_harder)?
    } else {
        "try harder off".to_string()
    };
    Ok(format!(
        "{} ({}); {writer}, {try_harder}",
        choice.name, choice.why
    ))
}

/// The settings conjure reads, or what is wrong with them:
/// `writer haiku, try harder sonnet, $3.00 a day`.
fn config_row(config: &sorcerer_core::Result<ConjureConfig>) -> Result<String, String> {
    let config = config.as_ref().map_err(ToString::to_string)?;
    let try_harder = if config.offers_try_harder() {
        format!("try harder {}", config.try_harder)
    } else {
        "try harder off".to_string()
    };
    Ok(format!(
        "writer {}, {try_harder}, ${:.2} a day",
        config.writer, config.budget_per_day
    ))
}

/// Whether each language the project uses has its tables read and its grammar and queries
/// loaded: `python (tests, imports)`, listing what conjure can use in each.
fn languages_row(project: &Project, cache: &GrammarCache) -> Result<String, String> {
    let names = project_languages(project, cache)?;
    if names.is_empty() {
        return Ok("no file here is in a language sorcerer knows".into());
    }
    let described: Vec<Result<String, String>> = names
        .iter()
        .map(|name| language_text(project, cache, name))
        .collect();
    let failed: Vec<String> = described
        .iter()
        .filter_map(|d| d.as_ref().err().cloned())
        .collect();
    if !failed.is_empty() {
        return Err(failed.join("; "));
    }
    Ok(described
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", "))
}

/// One language as `python (tests, imports)`: `tests` when its `[test]` names a test file,
/// `imports` when `[imports]` has templates and an `imports.scm` compiled.
fn language_text(project: &Project, cache: &GrammarCache, name: &str) -> Result<String, String> {
    let language = project
        .languages
        .get(name)
        .ok_or_else(|| format!("{name}: no such language"))?;
    let tests = test_table(language).map_err(|e| e.to_string())?;
    let imports = imports_table(language).map_err(|e| e.to_string())?;
    let loaded = project
        .languages
        .load(name, cache)
        .map_err(|e| format!("{name}: {e}"))?;
    let has_query = loaded.queries.iter().any(|(file, _)| file == "imports.scm");
    let mut reads: Vec<&str> = Vec::new();
    if tests.tests_for.is_some() {
        reads.push("tests");
    }
    if !imports.resolve.is_empty() && has_query {
        reads.push("imports");
    }
    let reads = if reads.is_empty() {
        "nothing conjure reads".to_string()
    } else {
        reads.join(", ")
    };
    Ok(format!("{name} ({reads})"))
}

/// The languages the project names in `[languages] names`, or, when it names none, those
/// covering a file in the project, sorted, each once.
fn project_languages(project: &Project, cache: &GrammarCache) -> Result<Vec<String>, String> {
    if !project.config.languages.is_empty() {
        return Ok(project.config.languages.clone());
    }
    let view = FilesView::new(Arc::new(project.clone()), cache.clone());
    let files = view.glob("**").map_err(|e| e.to_string())?;
    let used: BTreeSet<String> = files
        .iter()
        .filter_map(|f| project.languages.for_path(f).map(|l| l.name.clone()))
        .collect();
    Ok(used.into_iter().collect())
}

/// What conjure finds of scry's findings: `version 2, 3 open`, `no file`, or why it cannot
/// read them. Never a failure: conjure runs without scry's findings.
fn scry_findings_row(project: &Project) -> String {
    match read_findings(project) {
        Ok(FindingsRead::Found(findings)) => {
            let open = findings.iter().filter(|f| f.status == "open").count();
            format!("version 2, {open} open")
        }
        Ok(FindingsRead::NoFile) => "no file".into(),
        Ok(FindingsRead::OtherVersion(why)) => why.replace("this sorcerer reads", "conjure reads"),
        Err(e) => e.to_string(),
    }
}

/// How many rows the ledger holds, how many are conjure's, and how many lines did not read.
fn ledger_row(project: &Project) -> Result<String, String> {
    let ledger = JsonlLedger::at(project.state.join("ledger.jsonl"));
    let rows = ledger.read().map_err(|e| e.to_string())?;
    let ours = rows.errands.iter().filter(|e| e.product == PRODUCT).count();
    let unread = if rows.skipped > 0 {
        format!("; {} did not read", count(rows.skipped, "line"))
    } else {
        String::new()
    };
    Ok(format!(
        "{}, {ours} of them conjure's{unread}",
        count(rows.errands.len(), "row")
    ))
}

/// How many changes are recorded and how many lines were skipped: `3 changes, 0 lines skipped`.
fn records_row(project: &Project) -> Result<String, String> {
    let store = JsonlChangeStore::for_project(project).map_err(|e| e.to_string())?;
    let rows = store.read().map_err(|e| e.to_string())?;
    Ok(format!(
        "{}, {} skipped",
        count(rows.records.len(), "change"),
        count(rows.skipped, "line")
    ))
}

/// Whether the editor's server will conjure in the project: it needs no trust, it is trusted,
/// or it will ask, naming what in the config needs trust. Never a failure.
fn trust_row(project: &Project, list: Option<&TrustList>) -> Result<String, String> {
    let reasons = needs_trust(project);
    Ok(if reasons.is_empty() {
        "not needed".to_string()
    } else if list.is_some_and(|list| list.is_trusted(&project.root)) {
        "trusted".to_string()
    } else {
        let keys: Vec<&str> = reasons.iter().map(|r| r.key.as_str()).collect();
        format!("not trusted: the editor will ask ({})", keys.join(", "))
    })
}

/// A row as printed: `provider   ok    claude (…)`, or `FAIL` and why.
fn row_text(row: &DoctorRow) -> String {
    let name = row.name;
    match &row.result {
        Ok(found) => format!("{name:<13} ok    {found}\n"),
        Err(why) => format!("{name:<13} FAIL  {why}\n"),
    }
}
