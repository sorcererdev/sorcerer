//! `scry doctor`: whether scry can run in a project, a row each:
//! provider, Jev, languages, steering, store and ledger. It probes the real
//! adapters (the provider, Jev's key, the grammars) and so lives here, with
//! the wiring's other callers, not among the use cases. Reads only.

use std::io::Write;
use std::path::Path;

use anyhow::Result;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::versioned::Snapshot;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::adapters::jev::Jev;
use sorcerer_models::adapters::providers::{default_decider, default_provider_for};

use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::scry_toml::ScryToml;
use crate::domain::finding::Status;
use crate::domain::log_text::{bytes, count};
use crate::ports::finding_store::FindingStore;
use crate::ports::settings::Settings;

/// Prints each doctor row; 2 when any failed.
pub(super) fn doctor(
    project: &Project,
    env: &dyn Env,
    cache: &GrammarCache,
    out: &mut dyn Write,
) -> Result<i32> {
    let rows = doctor_rows(project, env, cache);
    let code = if rows.iter().any(|(_, row)| row.is_err()) {
        2
    } else {
        0
    };
    for (name, row) in &rows {
        write!(out, "{}", row_text(name, row))?;
    }
    Ok(code)
}

/// Whether scry can run in PROJECT, a row each: `provider`, `jev`,
/// `languages` (each language the project's files use loads), `steering`,
/// `store`, `ledger`. Reads only: nothing creates `.sorcerer/state`.
pub fn doctor_rows(
    project: &Project,
    env: &dyn Env,
    cache: &GrammarCache,
) -> Vec<(String, std::result::Result<String, String>)> {
    check_setup(
        project,
        env,
        cache,
        &JsonFindingStore::at(project.state.join("scry/findings.json")),
        &JsonlLedger::at(project.state.join("ledger.jsonl")),
        &ScryToml::for_project(project),
    )
}

/// The rows of [`doctor_rows`] over these ports.
fn check_setup(
    project: &Project,
    env: &dyn Env,
    cache: &GrammarCache,
    store: &dyn FindingStore,
    ledger: &dyn Ledger,
    settings: &dyn Settings,
) -> Vec<(String, Result<String, String>)> {
    vec![
        ("provider".into(), provider_row(project, env)),
        ("jev".into(), jev_row(project, env)),
        ("languages".into(), languages_row(project, cache)),
        ("steering".into(), steering_row(settings, &project.root)),
        ("store".into(), store_row(store)),
        ("ledger".into(), ledger_row(ledger)),
    ]
}

/// The model provider a review would use, and why.
fn provider_row(project: &Project, env: &dyn Env) -> Result<String, String> {
    default_provider_for(project, env)
        .map(|(_, c)| format!("{} ({})", c.name, c.why))
        .map_err(|e| e.to_string())
}

/// Whether Jev can be asked: its key, and the decider it would use.
fn jev_row(project: &Project, env: &dyn Env) -> Result<String, String> {
    match Jev::from_env(&project.config.decide, env, None) {
        Ok(_) => Ok(default_decider(&project.config.decide, env)
            .map(|c| format!("{} ({})", c.name, c.why))
            .unwrap_or_else(|| "jev".into())),
        Err(e) => Err(format!("{e}; scry does not run without Jev")),
    }
}

/// The steering understood: its rules, includes, context and size, and
/// its file.
fn steering_row(settings: &dyn Settings, root: &Path) -> Result<String, String> {
    let s = settings.steering().map_err(|e| e.to_string())?;
    Ok(match settings.steering_source() {
        None => "no .sorcerer/scry.toml: no rules".to_string(),
        Some(file) => format!(
            "{}, {}, {}, {}; {}",
            count(s.rules.len(), "rule"),
            count(s.includes.len(), "include"),
            if s.context.trim().is_empty() {
                "no context".to_string()
            } else {
                format!("context {}", bytes(s.context.trim().len()))
            },
            bytes(s.size()),
            file.strip_prefix(root).unwrap_or(&file).display()
        ),
    })
}

/// The store's findings and how many are open; an unreadable store fails.
fn store_row(store: &dyn FindingStore) -> Result<String, String> {
    match store.read().map_err(|e| e.to_string())? {
        Snapshot::Absent => Ok("no findings yet".into()),
        Snapshot::Current(s) => {
            let open = s
                .findings
                .iter()
                .filter(|f| f.status == Status::Open)
                .count();
            Ok(format!(
                "{}, {open} open",
                count(s.findings.len(), "finding")
            ))
        }
        Snapshot::Unreadable(why) => Err(format!(
            "{} cannot be read ({why}); scry does not run until it is fixed or moved aside",
            store.path().display()
        )),
    }
}

/// The ledger's rows, how many are scry's, and lines that did not read.
fn ledger_row(ledger: &dyn Ledger) -> Result<String, String> {
    let rows = ledger.read().map_err(|e| e.to_string())?;
    let scry = rows.errands.iter().filter(|e| e.product == "scry").count();
    let mut s = format!(
        "{}, {scry} of them scry's",
        count(rows.errands.len(), "row")
    );
    if rows.skipped > 0 {
        s.push_str(&format!("; {} did not read", count(rows.skipped, "line")));
    }
    Ok(s)
}

/// The project's languages (`[languages] names`, else those covering a
/// file in the project), each loaded through CACHE.
fn languages_row(project: &Project, cache: &GrammarCache) -> Result<String, String> {
    let names: Vec<String> = if project.config.languages.is_empty() {
        let view = FilesView::new(std::sync::Arc::new(project.clone()), cache.clone());
        let files = view.glob("**").map_err(|e| e.to_string())?;
        let mut used: Vec<String> = files
            .iter()
            .filter_map(|f| project.languages.for_path(f).map(|l| l.name.clone()))
            .collect();
        used.sort();
        used.dedup();
        used
    } else {
        project.config.languages.clone()
    };
    if names.is_empty() {
        return Ok("no file here is in a language sorcerer knows".into());
    }
    let failed: Vec<String> = names
        .iter()
        .filter_map(|n| match project.languages.load(n, cache) {
            Ok(_) => None,
            Err(e) => Some(format!("{n}: {e}")),
        })
        .collect();
    if failed.is_empty() {
        Ok(format!("{} load", names.join(", ")))
    } else {
        Err(failed.join("; "))
    }
}

// The command's text.

/// `provider   ok    claude (…)`, or `FAIL` and why.
fn row_text(name: &str, row: &Result<String, String>) -> String {
    match row {
        Ok(s) => format!("{name:<10} ok    {s}\n"),
        Err(s) => format!("{name:<10} FAIL  {s}\n"),
    }
}
