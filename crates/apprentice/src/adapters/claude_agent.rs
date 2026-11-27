//! The agent on Claude Code headless: one `claude -p` child per task, stream-json in and out,
//! in the project root, on the user's own login. Its writers and `AskUserQuestion` are denied,
//! the fences are deny rules, the medium tools come from `apprentice mcp`, the person's edits
//! from `apprentice hook`, and every permission question goes to the permission tool.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError};
use serde_json::{Value, json};
use sorcerer_core::adapters::platform::process::{self, Running as Process};
use sorcerer_core::domain::config::ProviderConfig;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::{Error, Result};

use crate::domain::agent_event::{AgentEvent, events_of_line};
use crate::domain::loopback_message::{PORT_VAR, TASK_VAR, TOKEN_VAR};
use crate::domain::mediums::{PERMISSION_TOOL, SERVER_NAME, TOOL_PREFIX};
use crate::ports::agent::{Agent, NextEvent, Running, TaskRequest};

/// The tools the agent may use unasked: the mediums, reading and searching, subagents, the web.
/// Reading and searching stop at the fences (the `Read` deny rules) and at the project's edge
/// (`blockReadsOutsideWorkingDirectories`). Naming `Grep` and `Glob` here also brings them back
/// on macOS and Linux, where Claude Code otherwise searches with `grep` and `find` through
/// `Bash`, which would ask the person about every search.
const ALLOWED_TOOLS: &str =
    "mcp__apprentice__*,Read,Grep,Glob,Agent,Task,TodoWrite,WebSearch,WebFetch";

/// The tools that run commands, asked about every time through the permission tool. Without
/// the rule Claude Code runs the commands it counts as read-only (`cat`, `grep -r`, `git show`)
/// unasked, and those can print a fenced file: `grep -r KEY .` names no fenced path, so no
/// `Read` rule stops it.
const ASKED_TOOLS: [&str; 3] = ["Bash", "PowerShell", "Monitor"];

/// The tools no task may use: every write goes through `write_file`, every question through
/// `ask`.
const DENIED_TOOLS: [&str; 5] = [
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
    "AskUserQuestion",
];

pub struct ClaudeAgent {
    runner: PathBuf,
    /// This program, which the agent runs as its MCP server and its hooks.
    me: PathBuf,
    fences: Fences,
    envs: Vec<(String, String)>,
}

impl ClaudeAgent {
    /// The agent through the provider's `runner`, else `claude` on PATH; `me` is the
    /// `apprentice` program the helpers run as.
    pub fn new(cfg: &ProviderConfig, me: PathBuf, fences: Fences) -> ClaudeAgent {
        ClaudeAgent {
            runner: cfg.runner.clone().unwrap_or_else(|| "claude".into()),
            me,
            fences,
            envs: Vec::new(),
        }
    }

    /// Extra environment variables for the child (tests point the fake at its script).
    #[doc(hidden)]
    pub fn with_env(mut self, envs: Vec<(String, String)>) -> ClaudeAgent {
        self.envs = envs;
        self
    }

    /// The flags, in order. No argument holds a newline (npm's `claude.cmd` on Windows refuses
    /// one) and the task's text goes on standard input, so the line stays short.
    pub fn args(&self, request: &TaskRequest) -> Vec<String> {
        let settings = self.settings().to_string();
        let mcp = self.mcp_config().to_string();
        let permission_tool = format!("{TOOL_PREFIX}{PERMISSION_TOOL}");
        [
            "-p",
            "--output-format",
            "stream-json",
            "--input-format",
            "stream-json",
            "--verbose",
            "--model",
            &request.model,
            "--max-budget-usd",
            &request.budget_usd.to_string(),
            "--max-turns",
            &request.max_turns.to_string(),
            "--setting-sources",
            "",
            "--settings",
            &settings,
            "--mcp-config",
            &mcp,
            "--strict-mcp-config",
            "--allowedTools",
            ALLOWED_TOOLS,
            "--permission-prompt-tool",
            &permission_tool,
            "--append-system-prompt",
            &request.system,
        ]
        .iter()
        .map(|arg| arg.to_string())
        .collect()
    }

    /// The hooks (the person's edits after each tool call and at the end of the turn) and the
    /// permissions: the deny rules (the writers, `AskUserQuestion`, and every fence as a `Read`
    /// rule, which fences Grep, Glob and the file commands Claude Code recognises, such as
    /// `cat .env`), every command asked about, and no file tool reading outside the project.
    fn settings(&self) -> Value {
        let hook = |event: &str| json!([{ "matcher": "*", "hooks": [{ "type": "command", "command": self.helper_command(&["hook", event]) }] }]);
        json!({
            "hooks": { "PostToolUse": hook("post-tool-use"), "Stop": hook("stop") },
            "permissions": {
                "deny": self.deny_rules(),
                "ask": ASKED_TOOLS,
                "blockReadsOutsideWorkingDirectories": true,
            },
        })
    }

    fn deny_rules(&self) -> Vec<String> {
        let fenced_files = self.fences.patterns().into_iter().flat_map(|pattern| {
            let root_form = pattern
                .strip_prefix("**/")
                .map(|root_form| format!("Read(./{root_form})"));
            // `.git/**` fences what is in `.git`; `Read(./.git)` fences a search of `.git` too
            let directory = pattern
                .strip_suffix("/**")
                .filter(|directory| !directory.contains('*'))
                .map(|directory| format!("Read(./{directory})"));
            std::iter::once(format!("Read(./{pattern})"))
                .chain(root_form)
                .chain(directory)
        });
        let fenced_links = self.fences.fenced_links().into_iter().flat_map(|link| {
            [
                format!("Read(./{})", forward_slashes(&link)),
                format!("Read(./{}/**)", forward_slashes(&link)),
            ]
        });
        let rules = DENIED_TOOLS
            .iter()
            .map(|tool| tool.to_string())
            .chain(fenced_files)
            .chain(fenced_links);
        rules.fold(Vec::new(), |mut once, rule| {
            if !once.contains(&rule) {
                once.push(rule);
            }
            once
        })
    }

    /// One stdio MCP server, `apprentice mcp`. The way back (port, token, task) is not in it:
    /// this is a command-line argument, which any local user can read (`ps`,
    /// `/proc/<pid>/cmdline`), so the server finds it in the environment it inherits from
    /// `claude`, which `command` sets.
    fn mcp_config(&self) -> Value {
        json!({
            "mcpServers": {
                SERVER_NAME: {
                    "command": forward_slashes(&self.me),
                    "args": ["mcp"],
                }
            }
        })
    }

    /// A command line the agent runs through its shell: this program, quoted, with `/` in its
    /// path on every platform. Claude Code's settings take a hook only as a shell string (no
    /// argv form), and on Windows it runs hooks in Git Bash, so one POSIX quoting serves all
    /// three platforms.
    fn helper_command(&self, args: &[&str]) -> String {
        format!(
            "{} {}",
            posix_quoted(&forward_slashes(&self.me)),
            args.join(" ")
        )
    }

    fn command(&self, request: &TaskRequest) -> Command {
        let mut command = Command::new(&self.runner);
        command
            .args(self.args(request))
            .current_dir(&request.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(self.envs.iter().map(|(name, value)| (name, value)))
            .envs(way_back_env(request))
            .env_remove("CLAUDECODE")
            .env("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1");
        command
    }
}

/// `text` as one shell word that no character in it can break: in single quotes, where only
/// `'` itself needs care (close the quote, write `\'`, open it again).
fn posix_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The way back, for `claude`'s environment; its MCP server and hooks inherit it. Every command
/// the agent runs inherits it too: the same trust as the agent, which may call the tools anyway.
fn way_back_env(request: &TaskRequest) -> [(&'static str, String); 3] {
    [
        (PORT_VAR, request.way_back.port.to_string()),
        (TOKEN_VAR, request.way_back.token.clone()),
        (TASK_VAR, request.task.clone()),
    ]
}

/// The first message on the agent's input: the task, as a user message.
pub fn user_message_line(text: &str) -> String {
    json!({ "type": "user", "message": { "role": "user", "content": text } }).to_string()
}

impl Agent for ClaudeAgent {
    /// Starts `claude` in a group of its own in the project root, writes the task as the first
    /// user message, and reads its output on a thread of its own.
    fn start(&self, request: &TaskRequest) -> Result<Box<dyn Running>> {
        let mut process = process::start(self.command(request)).map_err(|e| Error::Config {
            file: self.runner.clone(),
            why: format!("{} could not start: {e}", self.runner.display()),
        })?;
        let mut stdin = process.take_stdin().expect("piped");
        let started =
            writeln!(stdin, "{}", user_message_line(&request.prompt)).and_then(|()| stdin.flush());
        if let Err(e) = started {
            process.stop();
            return Err(Error::io(&self.runner, e));
        }
        let events = read_events(process.take_stdout().expect("piped"));
        drain(process.take_stderr().expect("piped"));
        Ok(Box::new(ClaudeRunning {
            process,
            stdin: Some(stdin),
            events,
        }))
    }
}

/// The agent's output, line by line, as events, on a thread of its own; the channel closes
/// when the output does.
fn read_events(stdout: impl Read + Send + 'static) -> Receiver<AgentEvent> {
    let (sender, receiver) = crossbeam_channel::unbounded();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            for event in events_of_line(&line) {
                if sender.send(event).is_err() {
                    return;
                }
            }
        }
    });
    receiver
}

/// Reads the error output away, so a full pipe never stalls the agent.
fn drain(stderr: impl Read + Send + 'static) {
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
    });
}

struct ClaudeRunning {
    process: Process,
    stdin: Option<ChildStdin>,
    events: Receiver<AgentEvent>,
}

impl Running for ClaudeRunning {
    fn next_event(&mut self, wait: Duration) -> NextEvent {
        match self.events.recv_timeout(wait) {
            Ok(event) => NextEvent::Event(event),
            Err(RecvTimeoutError::Timeout) => NextEvent::Waiting,
            Err(RecvTimeoutError::Disconnected) => NextEvent::Ended,
        }
    }

    fn close_input(&mut self) {
        self.stdin = None;
    }

    fn stop(&mut self) {
        self.stdin = None;
        self.process.stop();
    }
}

impl Drop for ClaudeRunning {
    fn drop(&mut self) {
        self.stdin = None;
        // a turn that ended lets the program exit by itself; give it a moment, then stop the
        // group, so nothing it started outlives the task
        for _ in 0..50 {
            if matches!(self.process.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.process.stop();
    }
}

/// Whether the runner can be found: an absolute path that exists, or a name on PATH.
pub fn runner_found(runner: &Path) -> bool {
    runner.is_absolute() && runner.exists() || which::which(runner).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::agent::WayBack;

    fn agent(root: &Path) -> ClaudeAgent {
        ClaudeAgent {
            runner: "claude".into(),
            me: PathBuf::from("/usr/local/bin/apprentice"),
            fences: Fences::new(root.into(), &["docs/plans".into()]).unwrap(),
            envs: Vec::new(),
        }
    }

    const AWKWARD_NAMES: [&str; 6] = [
        "plain",
        "with space",
        "dq\"uote",
        "dollar$HOME",
        "back`tick`",
        "it's",
    ];

    #[test]
    fn a_word_is_single_quoted_whatever_is_in_it() {
        assert_eq!(posix_quoted("/a b/x"), "'/a b/x'");
        assert_eq!(posix_quoted("it's"), "'it'\\''s'");
        assert_eq!(posix_quoted("a\"$`b"), "'a\"$`b'");
    }

    #[test]
    fn the_helper_command_quotes_an_awkward_path_with_forward_slashes() {
        let mut agent = agent(Path::new("/p"));
        agent.me = if cfg!(windows) {
            PathBuf::from(r"C:\Program Files\it's\$x\apprentice.exe")
        } else {
            PathBuf::from("C:/Program Files/it's/$x/apprentice.exe")
        };
        assert_eq!(
            agent.helper_command(&["hook", "stop"]),
            "'C:/Program Files/it'\\''s/$x/apprentice.exe' hook stop"
        );
    }

    /// The command goes through `sh -c` as claude runs it; the program must actually start.
    #[cfg(unix)]
    #[test]
    fn a_helper_with_an_awkward_name_runs_through_the_shell() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        for name in AWKWARD_NAMES {
            let folder = dir.path().join(name);
            std::fs::create_dir(&folder).unwrap();
            let exe = folder.join(name);
            std::fs::write(&exe, "#!/bin/sh\necho \"ran $*\"\n").unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut agent = agent(dir.path());
            agent.me = exe;
            let out = Command::new("sh")
                .arg("-c")
                .arg(agent.helper_command(&["hook", "stop"]))
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                "ran hook stop\n",
                "{name}"
            );
        }
    }

    fn request(root: &Path) -> TaskRequest {
        TaskRequest {
            task: "t1".into(),
            prompt: "do it\n".repeat(400),
            system: crate::domain::prompt::EDITOR_RULES.into(),
            model: "sonnet".into(),
            budget_usd: 2.0,
            max_turns: 60,
            root: root.into(),
            way_back: WayBack {
                port: 4242,
                token: "k".repeat(32),
            },
        }
    }

    /// On Windows `claude` is npm's `claude.cmd`: Rust refuses a newline in an argument sent to
    /// a `.cmd`, and `cmd.exe` cuts a command line at 8191 characters.
    #[test]
    fn no_argument_holds_a_newline_and_the_line_fits_cmd_exe() {
        let root = tempfile::TempDir::new().unwrap();
        let args = agent(root.path()).args(&request(root.path()));
        assert!(args.iter().all(|a| !a.contains(['\r', '\n'])), "{args:?}");
        let line_length: usize = args.iter().map(|a| a.len() + 1).sum();
        assert!(line_length < 8191, "{line_length}");
    }

    #[test]
    fn the_writers_are_denied_and_bash_is_left_to_the_permission_tool() {
        let root = tempfile::TempDir::new().unwrap();
        let agent = agent(root.path());
        let deny = agent.deny_rules();
        for tool in DENIED_TOOLS {
            assert!(deny.contains(&tool.to_string()), "{tool}");
        }
        assert!(
            deny.iter().any(|rule| rule.starts_with("Read(./")),
            "{deny:?}"
        );
        assert!(!ALLOWED_TOOLS.split(',').any(|tool| tool == "Bash"));
        let args = agent.args(&request(root.path()));
        let at = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(
            at("--permission-prompt-tool"),
            "mcp__apprentice__permission"
        );
        let mcp: Value = serde_json::from_str(&at("--mcp-config")).unwrap();
        assert_eq!(mcp["mcpServers"]["apprentice"]["args"], json!(["mcp"]));
        assert_eq!(mcp["mcpServers"]["apprentice"]["env"], Value::Null);
        let token = request(root.path()).way_back.token;
        assert!(!args.iter().any(|arg| arg.contains(&token)), "{args:?}");
        let settings: Value = serde_json::from_str(&at("--settings")).unwrap();
        assert_eq!(
            settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "'/usr/local/bin/apprentice' hook post-tool-use"
        );
    }
}
