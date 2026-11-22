//! The researcher a product asks: the project's default model provider, on Claude Code or on
//! sorcerer's own tool loop.

use std::sync::Arc;

use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::view::CodebaseView;

use crate::Result;
use crate::adapters::claude_code::ClaudeCodeResearcher;
use crate::adapters::providers::default_provider_for;
use crate::adapters::tool_loop::LoopResearcher;
use crate::ports::researcher::Researcher;

/// The researcher on the project's default model provider. A provider of
/// kind `claude` runs Claude Code; any other runs sorcerer's tool loop over
/// the provider, reading the project's files through `view`, within its
/// fences, and recording each call in its ledger.
///
/// Example: with `[model] provider = "claude"` and no `kind` set, the
/// researcher is Claude Code; with `provider = "local"` and
/// `[model.providers.local] kind = "openai"`, it is the tool loop over that
/// provider.
pub fn researcher_for(
    project: &Project,
    env: &dyn Env,
    view: Arc<dyn CodebaseView>,
) -> Result<Arc<dyn Researcher>> {
    let (provider, choice) = default_provider_for(project, env)?;
    let settings = project
        .config
        .model
        .providers
        .get(&choice.name)
        .cloned()
        .unwrap_or_default();
    // a provider is of the kind its name says unless `kind` says otherwise
    let kind = settings.kind.clone().unwrap_or_else(|| choice.name.clone());
    Ok(if kind == "claude" {
        Arc::new(ClaudeCodeResearcher::new(project, &settings)?)
    } else {
        Arc::new(LoopResearcher::new(
            provider,
            view,
            Fences::for_project(project)?,
            Arc::new(JsonlLedger::for_project(project)?),
        ))
    })
}
