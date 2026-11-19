//! Choosing a provider: by name from `[model.providers.<name>]` (its `kind`
//! defaults to the name), or the project's default (`default_provider`:
//! `[model] provider`, else `claude` on PATH, else `anthropic` with a key);
//! and the decider (`default_decider`: Jev when its key is set).

use std::sync::Arc;

use sorcerer_core::domain::config::{DecideConfig, ModelConfig};
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::anthropic::Anthropic;
use crate::adapters::claude::Claude;
use crate::adapters::openai::OpenAiCompatible;
use crate::ports::provider::Provider;
use crate::{Error, Result};

/// Which provider or decider was chosen, and why (for `doctor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderChoice {
    pub name: String,
    pub why: String,
}

fn choice(name: &str, why: &str) -> Option<ProviderChoice> {
    Some(ProviderChoice {
        name: name.to_string(),
        why: why.to_string(),
    })
}

/// The provider to use: `[model] provider` if set; else `claude` when it is
/// on PATH (a subscription); else `anthropic` when `ANTHROPIC_API_KEY` is
/// set; else none.
pub fn default_provider(cfg: &ModelConfig, env: &dyn Env) -> Option<ProviderChoice> {
    if let Some(p) = &cfg.provider {
        return choice(p, "set in [model]");
    }
    if env.which("claude").is_some() {
        return choice("claude", "claude is on PATH");
    }
    if env.var("ANTHROPIC_API_KEY").is_some() {
        return choice("anthropic", "ANTHROPIC_API_KEY is set");
    }
    None
}

/// The decider: Jev when its key (`[decide] key_env`, default `JEV_API_KEY`) is set.
pub fn default_decider(cfg: &DecideConfig, env: &dyn Env) -> Option<ProviderChoice> {
    let var = cfg.key_env.as_deref().unwrap_or("JEV_API_KEY");
    env.var(var)
        .and_then(|_| choice("jev", &format!("{var} is set")))
}

pub fn provider_named(project: &Project, name: &str, env: &dyn Env) -> Result<Arc<dyn Provider>> {
    let cfg = project
        .config
        .model
        .providers
        .get(name)
        .cloned()
        .unwrap_or_default();
    let kind = cfg.kind.clone().unwrap_or_else(|| name.to_string());
    Ok(match kind.as_str() {
        "anthropic" => Arc::new(Anthropic::from_config(&cfg, env)?),
        "claude" => Arc::new(Claude::from_config(&cfg)),
        "openai-compatible" => Arc::new(OpenAiCompatible::from_config(name, &cfg, env)?),
        other => {
            return Err(Error::Setup {
                why: format!(
                    "[model.providers.{name}]: unknown kind {other}; the kinds are anthropic, claude, openai-compatible"
                ),
            });
        }
    })
}

pub fn default_provider_for(
    project: &Project,
    env: &dyn Env,
) -> Result<(Arc<dyn Provider>, ProviderChoice)> {
    let choice = default_provider(&project.config.model, env).ok_or_else(|| Error::Setup {
        why: "no model provider: put claude on PATH or set ANTHROPIC_API_KEY".into(),
    })?;
    Ok((provider_named(project, &choice.name, env)?, choice))
}
