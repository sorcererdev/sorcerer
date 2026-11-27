//! `lackey doctor`: whether lackey can run in a project. A row for each thing it needs
//! (`claude` for tasks, the model provider for questions and claims, Jev for triage, its
//! settings, the ledger and the records), each `ok` or `FAIL` with what it found, and last
//! whether the editor's server needs the project trusted. It only reads.

use std::io::Write;

use anyhow::Result;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_models::adapters::providers::default_provider_for;

use crate::adapters::claude_agent::runner_found;
use crate::adapters::jsonl_task_store::JsonlTaskStore;
use crate::adapters::state_files::StateFiles;
use crate::adapters::wiring::claude_settings;
use crate::domain::config::{PRODUCT, lackey_config};
use crate::domain::log_text::usd;
use crate::ports::task_store::TaskStore;

/// One row: what was checked, and what was found or why it fails.
#[derive(Debug, Clone, PartialEq)]
pub struct DoctorRow {
    pub name: &'static str,
    pub result: std::result::Result<String, String>,
}

/// Prints the rows. The exit code is 2 when any row failed, else 0.
pub(super) fn doctor(project: &Project, env: &dyn Env, out: &mut dyn Write) -> Result<i32> {
    let rows = doctor_rows(project, env);
    for row in &rows {
        write!(out, "{}", row_text(row))?;
    }
    Ok(if rows.iter().any(|row| row.result.is_err()) {
        2
    } else {
        0
    })
}

/// The rows, in this order: `claude`, `provider`, `jev`, `config`, `ledger`, `records`,
/// `trust`.
pub fn doctor_rows(project: &Project, env: &dyn Env) -> Vec<DoctorRow> {
    let row = |name, result| DoctorRow { name, result };
    vec![
        row("claude", claude_row(project)),
        row("provider", Ok(provider_row(project, env))),
        row("jev", Ok(jev_row(env))),
        row("config", config_row(project)),
        row("ledger", ledger_row(project)),
        row("records", records_row(project)),
        row(
            "trust",
            Ok(trust_row(project, TrustList::user(env).as_ref())),
        ),
    ]
}

/// Tasks run on Claude Code headless: `claude` must be found.
fn claude_row(project: &Project) -> Result<String, String> {
    let runner = claude_settings(project)
        .runner
        .unwrap_or_else(|| "claude".into());
    if runner_found(&runner) {
        Ok(format!(
            "{} (tasks run on it, on your own login)",
            runner.display()
        ))
    } else {
        Err(format!(
            "{} is not found; tasks need Claude Code (https://claude.com/claude-code), questions and claims do not",
            runner.display()
        ))
    }
}

/// The provider questions and claims are answered on. Never a failure: without one, lackey
/// only offers tasks.
fn provider_row(project: &Project, env: &dyn Env) -> String {
    match default_provider_for(project, env) {
        Ok((_, choice)) if choice.name == "claude" => "claude (questions and claims)".to_string(),
        Ok((_, choice)) => format!(
            "{} (questions and claims; tasks still need claude: no other agent exists yet)",
            choice.name
        ),
        Err(e) => format!("none ({e}): questions and claims are not answered"),
    }
}

/// Jev classifies what you write; without its key nothing is classified, and only stubs
/// become offers.
fn jev_row(env: &dyn Env) -> String {
    match env.var("JEV_API_KEY") {
        Some(_) => "JEV_API_KEY is set (triage on idle)".to_string(),
        None => "JEV_API_KEY is not set: nothing you write is classified".to_string(),
    }
}

fn config_row(project: &Project) -> Result<String, String> {
    let config = lackey_config(project).map_err(|e| e.to_string())?;
    Ok(format!(
        "model {}, {} a task, {} a day, {} a day of triage",
        config.model,
        usd(config.budget_per_task),
        usd(config.budget_per_day),
        usd(config.triage_per_day)
    ))
}

fn ledger_row(project: &Project) -> Result<String, String> {
    let ledger = JsonlLedger::at(project.state.join("ledger.jsonl"));
    let rows = ledger.read().map_err(|e| e.to_string())?;
    let ours = rows.errands.iter().filter(|e| e.product == PRODUCT).count();
    Ok(format!(
        "{} rows, {ours} of them lackey's",
        rows.errands.len()
    ))
}

fn records_row(project: &Project) -> Result<String, String> {
    let state = StateFiles::for_project(project).map_err(|e| e.to_string())?;
    let rows = JsonlTaskStore::at(state.tasks())
        .read()
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{} tasks, {} lines skipped",
        rows.tasks.len(),
        rows.skipped
    ))
}

/// Whether the editor's server will run in the project. Never a failure.
fn trust_row(project: &Project, list: Option<&TrustList>) -> String {
    let reasons = needs_trust(project);
    if reasons.is_empty() {
        "not needed".to_string()
    } else if list.is_some_and(|list| list.is_trusted(&project.root)) {
        "trusted".to_string()
    } else {
        let keys: Vec<&str> = reasons.iter().map(|r| r.key.as_str()).collect();
        format!("not trusted: run `lackey trust` ({})", keys.join(", "))
    }
}

fn row_text(row: &DoctorRow) -> String {
    let name = row.name;
    match &row.result {
        Ok(found) => format!("{name:<9} ok    {found}\n"),
        Err(why) => format!("{name:<9} FAIL  {why}\n"),
    }
}
