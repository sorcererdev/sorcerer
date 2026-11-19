//! `scry doctor`.

use std::io::Write;

use anyhow::Result;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::scry_toml::ScryToml;
use crate::usecases::check_setup::check_setup;

/// Prints each doctor row; 2 when any failed.
pub(super) fn doctor(
    project: &Project,
    env: &dyn Env,
    cache: &GrammarCache,
    out: &mut dyn Write,
) -> Result<i32> {
    let rows = doctor_rows(project, env, cache);
    let mut code = 0;
    for (name, row) in rows {
        match row {
            Ok(s) => writeln!(out, "{name:<10} ok    {s}")?,
            Err(s) => {
                code = 2;
                writeln!(out, "{name:<10} FAIL  {s}")?
            }
        }
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
