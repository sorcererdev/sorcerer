//! The live wiring: the agent on `claude`, the stores under `.sorcerer/state/apprentice/`, the
//! ledger, the system clock, the project's context. The editor is the caller's: the server's,
//! or the command line's disk editor.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::config::ProviderConfig;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::claude_agent::ClaudeAgent;
use crate::adapters::json_permissions::JsonPermissions;
use crate::adapters::jsonl_task_store::JsonlTaskStore;
use crate::adapters::markdown_journal::MarkdownJournal;
use crate::adapters::project_context::FilesContext;
use crate::adapters::state_files::StateFiles;
use crate::adapters::system_clock::SystemClock;
use crate::domain::config::apprentice_config;
use crate::ports::agent::Agent;
use crate::ports::clock::Clock;
use crate::ports::editor::Editor;
use crate::usecases::run_task::TaskDeps;

/// Builds the agent for a project; the tests give a scripted one or the fake `claude`.
pub type WireAgent = Arc<dyn Fn(&Project) -> Result<Arc<dyn Agent>> + Send + Sync>;

/// The live agent: `claude` from `[model.providers.claude] runner`, else on PATH, with this
/// program as its helpers.
pub fn live_agent(project: &Project) -> Result<Arc<dyn Agent>> {
    let me = std::env::current_exe()?;
    Ok(Arc::new(ClaudeAgent::new(
        &claude_settings(project),
        me,
        Fences::for_project(project)?,
    )))
}

/// The `[model.providers.claude]` table, or the defaults.
pub fn claude_settings(project: &Project) -> ProviderConfig {
    project
        .config
        .model
        .providers
        .get("claude")
        .cloned()
        .unwrap_or_default()
}

/// Everything a task needs, over the project's files, with the given editor, agent and clock.
pub fn task_deps(
    project: &Project,
    env: &dyn Env,
    editor: Arc<dyn Editor>,
    agent: Arc<dyn Agent>,
    clock: Arc<dyn Clock>,
) -> Result<TaskDeps> {
    let state = StateFiles::for_project(project)?;
    let fences = Fences::for_project(project)?;
    Ok(TaskDeps {
        agent,
        editor,
        tasks: Arc::new(JsonlTaskStore::at(state.tasks())),
        journal: Arc::new(MarkdownJournal::at(state.task_dirs())),
        permissions: Arc::new(JsonPermissions::at(state.permissions())),
        ledger: Arc::new(JsonlLedger::for_project(project)?),
        clock,
        context: Arc::new(FilesContext {
            project: Arc::new(project.clone()),
            cache: GrammarCache::user(env),
            fences: fences.clone(),
        }),
        fences,
        root: project.root.clone(),
        config: apprentice_config(project)?,
    })
}

/// The system clock.
pub fn system_clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock)
}

/// Where the notes of a project go.
pub fn notes_dir(project: &Project) -> Result<PathBuf> {
    Ok(StateFiles::for_project(project)?.notes())
}
