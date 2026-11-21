//! `scry doctor`: whether scry can run in a project. It prints a row for
//! each thing scry needs (a model provider, Jev, the languages, the
//! steering, the findings store and the ledger), each `ok` or `FAIL` with
//! what it found, and last whether the editor's server needs the project
//! trusted.
//!
//! It tries the real adapters (the provider, Jev's key, the grammars), so it
//! lives here with the wiring's other callers, not among the use cases. It
//! only reads: nothing it does creates `.sorcerer/state`.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_core::domain::versioned::Snapshot;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::adapters::jev::Jev;
use sorcerer_models::adapters::providers::{default_decider, default_provider_for};

use super::format::relative_or_absolute;
use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::scry_toml::ScryToml;
use crate::domain::finding::Status;
use crate::domain::log_text::{bytes, count};
use crate::ports::finding_store::FindingStore;
use crate::ports::settings::Settings;

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

/// One row of `scry doctor`: what was checked (`provider`, `jev`,
/// `languages`, `steering`, `store` or `ledger`), and what was found, or
/// why it fails.
#[derive(Debug, Clone, PartialEq)]
pub struct DoctorRow {
    pub name: &'static str,
    pub result: std::result::Result<String, String>,
}

/// Whether scry can run in the project, a row for each thing it needs.
pub fn doctor_rows(project: &Project, env: &dyn Env, cache: &GrammarCache) -> Vec<DoctorRow> {
    check_setup(
        project,
        env,
        cache,
        &JsonFindingStore::at(project.state.join("scry/findings.json")),
        &JsonlLedger::at(project.state.join("ledger.jsonl")),
        &ScryToml::for_project(project),
    )
}

/// The rows of [`doctor_rows`], checked against these ports.
fn check_setup(
    project: &Project,
    env: &dyn Env,
    cache: &GrammarCache,
    store: &dyn FindingStore,
    ledger: &dyn Ledger,
    settings: &dyn Settings,
) -> Vec<DoctorRow> {
    let row = |name, result| DoctorRow { name, result };
    vec![
        row("git", git_row(env)),
        row("provider", provider_row(project, env)),
        row("jev", jev_row(project, env)),
        row("languages", languages_row(project, cache)),
        row("steering", steering_row(settings, &project.root)),
        row("store", store_row(store)),
        row("ledger", ledger_row(ledger)),
        row("trust", trust_row(project, TrustList::user(env).as_ref())),
    ]
}

/// Whether the editor's server will review the project: it needs no trust, it is trusted, or
/// it will ask, naming what in the config needs trust. Never a failure: the command line
/// runs either way.
///
/// Example: `not trusted: the editor will ask ([model.providers.mine] runner)`.
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

/// Where `git` was found: scry reads your changes through it. A missing git
/// fails with where to get it.
///
/// Example: `/usr/bin/git`.
fn git_row(env: &dyn Env) -> Result<String, String> {
    env.which("git")
        .map(|path| path.display().to_string())
        .ok_or_else(|| {
            "git is not on PATH; scry reads your changes through git. Install it: \
             https://git-scm.com/downloads"
                .to_string()
        })
}

/// The model provider a review would use, and why it was chosen:
/// `claude (claude is on PATH)`.
fn provider_row(project: &Project, env: &dyn Env) -> Result<String, String> {
    default_provider_for(project, env)
        .map(|(_, c)| format!("{} ({})", c.name, c.why))
        .map_err(|e| e.to_string())
}

/// Whether Jev can be asked (its key is set), and the decider it would
/// use. scry does not run without Jev.
fn jev_row(project: &Project, env: &dyn Env) -> Result<String, String> {
    match Jev::from_env(&project.config.decide, env, None) {
        Ok(_) => Ok(default_decider(env)
            .map(|c| format!("{} ({})", c.name, c.why))
            .unwrap_or_else(|| "jev".into())),
        Err(e) => Err(format!(
            "{e}; scry does not run without Jev. Get a key at https://typesafe.ai"
        )),
    }
}

/// The steering scry read, in short: its rules, includes, context and
/// size, and its file. A steering file with a mistake in it fails.
///
/// Example: `5 rules, 1 include, context 312 B, 2.0 KB; .sorcerer/scry.toml`.
fn steering_row(settings: &dyn Settings, root: &Path) -> Result<String, String> {
    let steering = settings.steering().map_err(|e| e.to_string())?;
    let Some(file) = settings.steering_source() else {
        return Ok("no .sorcerer/scry.toml: no rules".to_string());
    };
    let context = steering.context.trim();
    let context_size = if context.is_empty() {
        "no context".to_string()
    } else {
        format!("context {}", bytes(context.len()))
    };
    Ok(format!(
        "{}, {}, {context_size}, {}; {}",
        count(steering.rules.len(), "rule"),
        count(steering.includes.len(), "include"),
        bytes(steering.size()),
        relative_or_absolute(&file, root)
    ))
}

/// How many findings the store holds and how many are open: `12
/// findings, 3 open`. A store that cannot be read fails, since scry will
/// not run over it.
fn store_row(store: &dyn FindingStore) -> Result<String, String> {
    match store.read().map_err(|e| e.to_string())? {
        Snapshot::Absent => Ok("no findings yet".into()),
        Snapshot::Current(stored) => {
            let open = stored
                .findings
                .iter()
                .filter(|f| f.status == Status::Open)
                .count();
            Ok(format!(
                "{}, {open} open",
                count(stored.findings.len(), "finding")
            ))
        }
        Snapshot::Unreadable(why) => Err(format!(
            "{} cannot be read ({why}); scry does not run until it is fixed or moved aside",
            store.path().display()
        )),
    }
}

/// How many rows the ledger holds, how many are scry's, and how many
/// lines did not read: `40 rows, 31 of them scry's; 1 line did not read`.
fn ledger_row(ledger: &dyn Ledger) -> Result<String, String> {
    let rows = ledger.read().map_err(|e| e.to_string())?;
    let scry = rows.errands.iter().filter(|e| e.product == "scry").count();
    let unread = if rows.skipped > 0 {
        format!("; {} did not read", count(rows.skipped, "line"))
    } else {
        String::new()
    };
    Ok(format!(
        "{}, {scry} of them scry's{unread}",
        count(rows.errands.len(), "row")
    ))
}

/// Whether each of the project's languages loads (its grammar builds and
/// its queries read): `python, rust load`, or each that fails and why.
fn languages_row(project: &Project, cache: &GrammarCache) -> Result<String, String> {
    let names = project_languages(project, cache)?;
    if names.is_empty() {
        return Ok("no file here is in a language sorcerer knows".into());
    }
    let failed: Vec<String> = names
        .iter()
        .filter_map(|name| match project.languages.load(name, cache) {
            Ok(_) => None,
            Err(e) => Some(format!("{name}: {e}")),
        })
        .collect();
    if failed.is_empty() {
        Ok(format!("{} load", names.join(", ")))
    } else {
        Err(failed.join("; "))
    }
}

/// The languages the project names in `[languages] names`, or, when it
/// names none, those covering a file in the project, sorted, each once.
fn project_languages(project: &Project, cache: &GrammarCache) -> Result<Vec<String>, String> {
    if !project.config.languages.is_empty() {
        return Ok(project.config.languages.clone());
    }
    let view = FilesView::new(std::sync::Arc::new(project.clone()), cache.clone());
    let files = view.glob("**").map_err(|e| e.to_string())?;
    let used: BTreeSet<String> = files
        .iter()
        .filter_map(|f| project.languages.for_path(f).map(|l| l.name.clone()))
        .collect();
    Ok(used.into_iter().collect())
}

// The command's text.

/// A row as printed: `provider   ok    claude (…)`, or `FAIL` and why.
fn row_text(row: &DoctorRow) -> String {
    let name = row.name;
    match &row.result {
        Ok(found) => format!("{name:<10} ok    {found}\n"),
        Err(why) => format!("{name:<10} FAIL  {why}\n"),
    }
}
