//! A researcher through `claude -p`: the CLI runs the tool loop with its
//! own Read, Grep and Glob, in the project root. Fences are enforced by
//! `claude`'s deny rules (a `Read(...)` rule fences Grep and Glob too,
//! measured 2026-11-08), not only by the prompt.

use std::path::PathBuf;

use serde_json::json;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::config::ProviderConfig;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::ledger::Ledger;

use crate::adapters::claude::{ClaudeReply, ClaudeRun};
use crate::domain::research::{Research, Researched};
use crate::ports::researcher::Researcher;
use crate::{Error, Result};

const FENCE_PROMPT: &str = include_str!("../../prompts/fence.md");

pub struct ClaudeCodeResearcher {
    runner: PathBuf,
    root: PathBuf,
    fences: Fences,
    ledger: JsonlLedger,
    envs: Vec<(String, String)>,
}

impl ClaudeCodeResearcher {
    pub fn new(project: &Project, cfg: &ProviderConfig) -> Result<ClaudeCodeResearcher> {
        Ok(ClaudeCodeResearcher {
            runner: cfg.runner.clone().unwrap_or_else(|| "claude".into()),
            root: project.root.clone(),
            fences: Fences::for_project(project)?,
            ledger: JsonlLedger::for_project(project)?,
            envs: Vec::new(),
        })
    }

    /// Extra environment variables for the child (tests).
    #[doc(hidden)]
    pub fn with_env(mut self, envs: Vec<(String, String)>) -> ClaudeCodeResearcher {
        self.envs = envs;
        self
    }

    /// `claude -p` with only the research's tools allowed, the fences as deny rules, web
    /// search and fetch denied, and the fence prompt appended to the system prompt.
    fn args(&self, r: &Research) -> Vec<String> {
        let tools = r
            .tools
            .iter()
            .map(|t| t.claude_name())
            .collect::<Vec<_>>()
            .join(",");
        let settings = json!({"permissions": {"deny": self.deny_rules()}}).to_string();
        let cap = r.cap_usd.to_string();
        let prompt = FENCE_PROMPT
            .trim()
            .replace("{fences}", &self.fences.patterns().join(", "));
        [
            "-p",
            "--output-format",
            "json",
            "--model",
            &r.model,
            // the call's cap: `claude -p` stops itself once it has spent this much
            "--max-budget-usd",
            &cap,
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

    /// What `claude` may not do. A fence such as `**/.env` is denied in both forms, `**/.env`
    /// and the root's `.env`, as the measured rules did. `claude` reads through symlinks, so
    /// a link out of the root or onto a fenced file is denied by its own name, and so is
    /// everything under it (measured 2026-11-17).
    fn deny_rules(&self) -> Vec<String> {
        let fenced_files = self.fences.patterns().into_iter().flat_map(|pattern| {
            let root_form = pattern
                .strip_prefix("**/")
                .map(|root_form| format!("Read(./{root_form})"));
            std::iter::once(format!("Read(./{pattern})")).chain(root_form)
        });
        let fenced_links = self.fences.fenced_links().into_iter().flat_map(|link| {
            [
                format!("Read(./{})", forward_slashes(&link)),
                format!("Read(./{}/**)", forward_slashes(&link)),
            ]
        });
        fenced_files
            .chain(fenced_links)
            .chain(["WebSearch".to_string(), "WebFetch".to_string()])
            .collect()
    }
}

impl Researcher for ClaudeCodeResearcher {
    /// Runs `claude` in the project root on the research prompt and records the call in
    /// the ledger, whether it worked or not, before returning its answer.
    fn research(&self, r: Research, cancel: &Cancel) -> Result<Researched> {
        let result = ClaudeRun {
            runner: &self.runner,
            args: self.args(&r),
            cwd: Some(&self.root),
            prompt: &r.prompt,
            timeout: r.timeout,
            envs: &self.envs,
        }
        .run(cancel);
        let errand = errand(&r, &result);
        self.ledger.append(&errand)?;
        Ok(Researched {
            answer: result?.text,
            errand,
            reads: Vec::new(),
        })
    }
}

/// The ledger line for one research call. A call that ran out of time was probably paid
/// for, so it is charged the research's whole cap and its whole time limit.
///
/// Example: a research capped at $0.50 that times out after 300 seconds is recorded as
/// costing $0.50 and taking 300 seconds; one that fails to start costs nothing.
fn errand(r: &Research, result: &Result<ClaudeReply>) -> Errand {
    let failed = Errand {
        at: chrono::Utc::now(),
        product: r.product.clone(),
        verb: r.verb.clone(),
        model: r.model.clone(),
        provider: "claude".into(),
        cost_usd: None,
        input_tokens: 0,
        output_tokens: 0,
        seconds: 0.0,
        ok: false,
        note: None,
    };
    match result {
        Ok(reply) => Errand {
            cost_usd: reply.cost_usd,
            input_tokens: reply.usage.input_tokens,
            output_tokens: reply.usage.output_tokens,
            seconds: reply.seconds,
            ok: true,
            ..failed
        },
        Err(e @ Error::Timeout { .. }) => Errand {
            cost_usd: Some(r.cap_usd),
            seconds: r.timeout.as_secs_f64(),
            note: Some(e.to_string()),
            ..failed
        },
        Err(e) => Errand {
            note: Some(e.to_string()),
            ..failed
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::domain::research::Tool;

    use super::*;

    /// On Windows `claude` is npm's `claude.cmd`: Rust refuses a newline in an argument sent to
    /// a `.cmd`, and `cmd.exe` cuts a command line at 8191 characters.
    #[test]
    fn no_argument_holds_a_newline_and_the_line_fits_cmd_exe() {
        let root = tempfile::TempDir::new().unwrap();
        let researcher = ClaudeCodeResearcher {
            runner: "claude".into(),
            root: root.path().into(),
            fences: Fences::new(root.path().into(), &["docs/plans".into(), "notes".into()])
                .unwrap(),
            ledger: JsonlLedger::at(root.path().join("ledger.jsonl")),
            envs: Vec::new(),
        };
        let research = Research {
            product: "scry".into(),
            verb: "review".into(),
            prompt: "review this unit\n".repeat(100),
            model: "sonnet".into(),
            tools: vec![Tool::Read, Tool::Grep, Tool::Glob],
            cap_usd: 0.25,
            timeout: Duration::from_secs(60),
        };
        let args = researcher.args(&research);
        assert!(args.iter().all(|a| !a.contains(['\r', '\n'])), "{args:?}");
        let line_length: usize = args.iter().map(|a| a.len() + 1).sum();
        assert!(line_length < 8191, "{line_length}");
    }
}
