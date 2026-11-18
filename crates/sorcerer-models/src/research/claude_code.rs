//! A researcher through `claude -p`: the CLI runs the tool loop with its
//! own Read, Grep and Glob, in the project root. Fences are enforced by
//! `claude`'s deny rules (a `Read(...)` rule fences Grep and Glob too,
//! measured in olifant 2026-11-08), not only by the prompt. Ported from
//! olifant's `runnerArgs` for a code check (`Server/Errand.hs`).

use std::path::PathBuf;

use serde_json::json;
use sorcerer_core::config::ProviderConfig;
use sorcerer_core::fences::Fences;
use sorcerer_core::ledger::{Errand, Ledger};
use sorcerer_core::{Cancel, Project};

use super::{Research, Researched, Researcher};
use crate::provider::claude::run_claude;
use crate::{Error, Result};

const FENCE_PROMPT: &str = include_str!("../../prompts/fence.md");

pub struct ClaudeCodeResearcher {
    runner: PathBuf,
    root: PathBuf,
    fences: Fences,
    ledger: Ledger,
    envs: Vec<(String, String)>,
}

impl ClaudeCodeResearcher {
    pub fn new(project: &Project, cfg: &ProviderConfig) -> Result<ClaudeCodeResearcher> {
        Ok(ClaudeCodeResearcher {
            runner: cfg.runner.clone().unwrap_or_else(|| "claude".into()),
            root: project.root.clone(),
            fences: Fences::for_project(project)?,
            ledger: Ledger::for_project(project)?,
            envs: Vec::new(),
        })
    }

    /// Extra environment variables for the child (tests).
    #[doc(hidden)]
    pub fn with_env(mut self, envs: Vec<(String, String)>) -> ClaudeCodeResearcher {
        self.envs = envs;
        self
    }

    fn args(&self, r: &Research) -> Vec<String> {
        let tools = r
            .tools
            .iter()
            .map(|t| t.claude_name())
            .collect::<Vec<_>>()
            .join(",");
        let patterns = self.fences.patterns();
        let deny: Vec<String> = patterns
            .iter()
            .map(|p| format!("Read(./{p})"))
            .chain(["WebSearch".to_string(), "WebFetch".to_string()])
            .collect();
        let settings = json!({"permissions": {"deny": deny}}).to_string();
        let prompt = FENCE_PROMPT
            .trim()
            .replace("{fences}", &patterns.join(", "));
        [
            "-p",
            "--output-format",
            "json",
            "--model",
            &r.model,
            "--permission-prompts",
            "none",
            "--strict-mcp-config",
            "--setting-sources",
            "",
            "--tools",
            &tools,
            "--allowedTools",
            &tools,
            "--settings",
            &settings,
            "--append-system-prompt",
            &prompt,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }
}

impl Researcher for ClaudeCodeResearcher {
    fn research(&self, r: Research, cancel: &Cancel) -> Result<Researched> {
        let result = run_claude(
            &self.runner,
            &self.args(&r),
            Some(&self.root),
            &r.prompt,
            r.timeout,
            cancel,
            &self.envs,
        );
        let errand = match &result {
            Ok(reply) => Errand {
                at: chrono::Utc::now(),
                product: r.product.clone(),
                verb: r.verb.clone(),
                model: r.model.clone(),
                provider: "claude".into(),
                cost_usd: reply.cost_usd,
                input_tokens: reply.usage.input_tokens,
                output_tokens: reply.usage.output_tokens,
                seconds: reply.seconds,
                ok: true,
                note: None,
            },
            Err(e) => Errand {
                at: chrono::Utc::now(),
                product: r.product.clone(),
                verb: r.verb.clone(),
                model: r.model.clone(),
                provider: "claude".into(),
                // a call that ran out of time was probably paid for
                cost_usd: matches!(e, Error::Timeout { .. }).then_some(r.cap_usd),
                input_tokens: 0,
                output_tokens: 0,
                seconds: if matches!(e, Error::Timeout { .. }) {
                    r.timeout.as_secs_f64()
                } else {
                    0.0
                },
                ok: false,
                note: Some(e.to_string()),
            },
        };
        self.ledger.append(&errand)?;
        let reply = result?;
        Ok(Researched {
            answer: reply.text,
            errand,
            reads: Vec::new(),
        })
    }
}
