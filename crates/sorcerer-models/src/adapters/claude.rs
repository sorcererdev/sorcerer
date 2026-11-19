//! Claude through `claude -p` (Claude Code on a subscription: no API key).
//! The flags: JSON out, no tools,
//! no session saved, no settings, CLAUDE.md files off, and `CLAUDECODE`
//! unset so a run from inside Claude Code is not refused as nested.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use sorcerer_core::Cancel;
use sorcerer_core::domain::config::ProviderConfig;

use crate::domain::model::{Completion, Content, ModelId, Reply, StopReason, Usage};
use crate::ports::provider::Provider;
use crate::{Error, Result};

pub struct Claude {
    runner: PathBuf,
    envs: Vec<(String, String)>,
}

impl Claude {
    /// The table's `runner`, or `claude` on PATH.
    pub fn from_config(cfg: &ProviderConfig) -> Claude {
        Claude {
            runner: cfg.runner.clone().unwrap_or_else(|| "claude".into()),
            envs: Vec::new(),
        }
    }

    /// A runner with extra environment variables for the child (tests).
    #[doc(hidden)]
    pub fn with_env(runner: PathBuf, envs: Vec<(String, String)>) -> Claude {
        Claude { runner, envs }
    }
}

/// What `claude -p --output-format json` printed.
#[derive(Debug, Clone)]
pub(crate) struct ClaudeReply {
    pub text: String,
    pub cost_usd: Option<f64>,
    pub seconds: f64,
    pub usage: Usage,
}

/// Runs RUNNER with ARGS in CWD, STDIN as its input, under TIMEOUT and
/// CANCEL (either kills it). `CLAUDECODE` is removed and CLAUDE.md files
/// are turned off for the child, after ENVS are applied.
pub(crate) fn run_claude(
    runner: &Path,
    args: &[String],
    cwd: Option<&Path>,
    stdin: &str,
    timeout: Duration,
    cancel: &Cancel,
    envs: &[(String, String)],
) -> Result<ClaudeReply> {
    let name = runner.display().to_string();
    let mut cmd = Command::new(runner);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.envs(envs.iter().map(|(k, v)| (k, v)));
    cmd.env_remove("CLAUDECODE")
        .env("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1");
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    // its own process group, so a kill reaches whatever it started
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| Error::Subprocess {
        runner: name.clone(),
        why: e.to_string(),
    })?;
    let mut input = child.stdin.take().expect("piped");
    let stdin = stdin.to_string();
    std::thread::spawn(move || {
        let _ = input.write_all(stdin.as_bytes());
    });
    let mut out = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let mut err = child.stderr.take().expect("piped");
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| Error::Subprocess {
            runner: name.clone(),
            why: e.to_string(),
        })? {
            break status;
        }
        if cancel.is_cancelled() || Instant::now() >= deadline {
            kill_group(&mut child);
            return Err(if cancel.is_cancelled() {
                Error::Cancelled
            } else {
                Error::Timeout {
                    provider: "claude".into(),
                    seconds: timeout.as_secs(),
                }
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    let bad = |why: String| Error::Reply {
        provider: "claude".into(),
        why,
    };
    let v: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        let said = if stdout.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        bad(format!(
            "exited {status} with no JSON reply: {}",
            said.chars().take(500).collect::<String>()
        ))
    })?;
    let text = v["result"].as_str().unwrap_or_default().to_string();
    if v["is_error"].as_bool().unwrap_or(false) {
        return Err(bad(text));
    }
    Ok(ClaudeReply {
        text,
        cost_usd: v["total_cost_usd"].as_f64(),
        seconds: v["duration_ms"].as_f64().map_or(0.0, |ms| ms / 1000.0),
        usage: Usage {
            input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
        },
    })
}

/// Kills CHILD and its process group.
fn kill_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

impl Provider for Claude {
    fn name(&self) -> &str {
        "claude"
    }

    /// `claude` takes the aliases itself.
    fn resolve(&self, alias: &str) -> Result<ModelId> {
        Ok(ModelId(alias.into()))
    }

    fn complete(&self, req: Completion, cancel: &Cancel) -> Result<Reply> {
        let args: Vec<String> = [
            "-p",
            "--output-format",
            "json",
            "--model",
            &req.model.0,
            "--permission-prompts",
            "none",
            "--system-prompt",
            &req.system,
            "--tools",
            "",
            "--no-session-persistence",
            "--setting-sources",
            "",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let prompt = req
            .messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|c| match c {
                Content::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let r = run_claude(
            &self.runner,
            &args,
            None,
            &prompt,
            req.timeout,
            cancel,
            &self.envs,
        )?;
        Ok(Reply {
            text: r.text,
            tool_calls: Vec::new(),
            usage: r.usage,
            cost_usd: r.cost_usd,
            stop: StopReason::End,
            seconds: r.seconds,
        })
    }

    /// Tool use goes through the claude researcher, never `complete`.
    fn supports_tools(&self) -> bool {
        false
    }
}
