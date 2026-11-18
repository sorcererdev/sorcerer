//! Choosing a provider: by name from `[model.providers.<name>]` (its `kind`
//! defaults to the name), or the project's default (core's
//! `default_provider`: `[model] provider`, else `claude` on PATH, else
//! `anthropic` with a key).

use std::sync::Arc;

use sorcerer_core::Project;
use sorcerer_core::env::Env;
use sorcerer_core::project::{ProviderChoice, default_provider};

use crate::provider::Provider;
use crate::provider::anthropic::Anthropic;
use crate::provider::claude::Claude;
use crate::provider::openai::OpenAiCompatible;
use crate::{Error, Result};

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
