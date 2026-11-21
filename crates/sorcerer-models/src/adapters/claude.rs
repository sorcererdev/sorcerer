//! Claude through `claude -p` (Claude Code on a subscription: no API key).
//! The flags: JSON out, no tools,
//! no session saved, no settings, CLAUDE.md files off, and `CLAUDECODE`
//! unset so a run from inside Claude Code is not refused as nested.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::platform::process::{self, Running};
use sorcerer_core::domain::config::ProviderConfig;

use crate::domain::model::{Completion, Content, ModelId, Reply, StopReason, Usage};
use crate::ports::provider::Provider;
use crate::{Error, Result};

pub struct Claude {
    runner: PathBuf,
    envs: Vec<(String, String)>,
}

impl Claude {
    /// The program the table's `runner` names, or `claude` on PATH.
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

/// One run of `claude`: the program, its arguments, the folder it runs in (the current
/// one when none), the prompt it reads on its input, how long it may take, and extra
/// environment variables (tests use these to script a fake `claude`).
pub(crate) struct ClaudeRun<'a> {
    pub runner: &'a Path,
    pub args: Vec<String>,
    pub cwd: Option<&'a Path>,
    pub prompt: &'a str,
    pub timeout: Duration,
    pub envs: &'a [(String, String)],
}

impl ClaudeRun<'_> {
    /// Runs `claude` and reads the JSON it prints. A cancel or the timeout kills it and
    /// everything it started. Output that is not JSON is an error quoting the first 500
    /// characters of what it printed (its error output when it printed nothing else),
    /// and so is a reply that says it is an error.
    ///
    /// Example: `claude` exits 1 printing only "Not logged in" as an error: the error
    /// reads "exited exit status: 1 with no JSON reply: Not logged in".
    pub(crate) fn run(&self, cancel: &Cancel) -> Result<ClaudeReply> {
        let mut runner = self.start()?;
        let printing = Printing::start(&mut runner, self.prompt);
        let status = self.wait_for_exit(&mut runner, cancel)?;
        reply_from(&printing.finish(), status)
    }

    fn runner_name(&self) -> String {
        self.runner.display().to_string()
    }

    /// Starts `claude` in a group of its own (a process group on Unix, a job object on
    /// Windows), so stopping it reaches whatever it started. `CLAUDECODE` is removed, so a run
    /// from inside Claude Code is not refused as nested, and CLAUDE.md files are turned off.
    fn start(&self) -> Result<Running> {
        let mut command = Command::new(self.runner);
        command
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(self.envs.iter().map(|(name, value)| (name, value)))
            .env_remove("CLAUDECODE")
            .env("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1");
        if let Some(folder) = self.cwd {
            command.current_dir(folder);
        }
        process::start(command).map_err(|e| Error::Subprocess {
            runner: self.runner_name(),
            why: e.to_string(),
        })
    }

    /// Checks every 50 milliseconds whether `claude` has exited, stopping it and everything
    /// it started on a cancel or once the timeout has passed.
    fn wait_for_exit(&self, runner: &mut Running, cancel: &Cancel) -> Result<ExitStatus> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let exited = runner.try_wait().map_err(|e| Error::Subprocess {
                runner: self.runner_name(),
                why: e.to_string(),
            })?;
            if let Some(status) = exited {
                return Ok(status);
            }
            if cancel.is_cancelled() {
                runner.stop();
                return Err(Error::Cancelled);
            }
            if Instant::now() >= deadline {
                runner.stop();
                return Err(Error::Timeout {
                    provider: "claude".into(),
                    seconds: self.timeout.as_secs(),
                });
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// `claude`'s output and error output, each read on a thread of its own so a full pipe
/// never stalls it.
struct Printing {
    stdout: JoinHandle<String>,
    stderr: JoinHandle<String>,
}

/// Everything `claude` printed, once it has exited.
struct Printed {
    stdout: String,
    stderr: String,
}

impl Printing {
    /// Writes the prompt to the child's input and starts reading what it prints.
    fn start(runner: &mut Running, prompt: &str) -> Printing {
        let mut input = runner.take_stdin().expect("piped");
        let prompt = prompt.to_string();
        std::thread::spawn(move || {
            let _ = input.write_all(prompt.as_bytes());
        });
        Printing {
            stdout: read_all_on_its_own_thread(runner.take_stdout().expect("piped")),
            stderr: read_all_on_its_own_thread(runner.take_stderr().expect("piped")),
        }
    }

    fn finish(self) -> Printed {
        Printed {
            stdout: self.stdout.join().unwrap_or_default(),
            stderr: self.stderr.join().unwrap_or_default(),
        }
    }
}

fn read_all_on_its_own_thread(mut pipe: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

fn reply_from(printed: &Printed, status: ExitStatus) -> Result<ClaudeReply> {
    let stdout = printed.stdout.trim();
    let reply: Value = serde_json::from_str(stdout).map_err(|_| {
        let said = if stdout.is_empty() {
            printed.stderr.trim()
        } else {
            stdout
        };
        reply_error(format!(
            "exited {status} with no JSON reply: {}",
            said.chars().take(500).collect::<String>()
        ))
    })?;
    let text = reply["result"].as_str().unwrap_or_default().to_string();
    if reply["is_error"].as_bool().unwrap_or(false) {
        return Err(reply_error(text));
    }
    Ok(ClaudeReply {
        text,
        cost_usd: reply["total_cost_usd"].as_f64(),
        seconds: reply["duration_ms"].as_f64().map_or(0.0, |ms| ms / 1000.0),
        usage: Usage {
            input_tokens: reply["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: reply["usage"]["output_tokens"].as_u64().unwrap_or(0),
        },
    })
}

fn reply_error(why: String) -> Error {
    Error::Reply {
        provider: "claude".into(),
        why,
    }
}

impl Provider for Claude {
    fn name(&self) -> &str {
        "claude"
    }

    /// `claude` takes the aliases itself.
    fn resolve(&self, alias: &str) -> Result<ModelId> {
        Ok(ModelId(alias.into()))
    }

    /// Runs `claude -p` with no tools, the conversation's text as one prompt.
    fn complete(&self, req: Completion, cancel: &Cancel) -> Result<Reply> {
        let reply = ClaudeRun {
            runner: &self.runner,
            args: completion_args(&req),
            cwd: None,
            prompt: &text_of_every_message(&req),
            timeout: req.timeout,
            envs: &self.envs,
        }
        .run(cancel)?;
        Ok(Reply {
            text: reply.text,
            tool_calls: Vec::new(),
            usage: reply.usage,
            cost_usd: reply.cost_usd,
            stop: StopReason::End,
            seconds: reply.seconds,
        })
    }

    /// Tool use goes through the claude researcher, never `complete`.
    fn supports_tools(&self) -> bool {
        false
    }
}

fn completion_args(req: &Completion) -> Vec<String> {
    [
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
    .collect()
}

/// The text of every message, joined by a blank line: `claude -p` takes one prompt.
fn text_of_every_message(req: &Completion) -> String {
    req.messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            Content::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
