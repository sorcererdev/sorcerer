//! A fake `claude -p --input-format stream-json --output-format stream-json` for the harness
//! tests. It writes its arguments and its first input line to files, reads a script, and plays
//! it: it really starts the MCP server named in `--mcp-config` and calls tools through it, and
//! really runs the hooks named in `--settings` after each tool call and at the end, logging
//! every result to a file of JSON lines.
//!
//! Like the real one, it stops playing its steps when a `PostToolUse` hook answers
//! `{"continue": false}`, and it runs a command it counts as read-only (`cat`, `grep`, `git`, …)
//! without asking the permission tool, unless the settings' `permissions.ask` names `Bash`.
//!
//! The script (`FAKE_LACKEY_SCRIPT`, JSON): `{"steps": [...], "cost": 0.71, "session":
//! "s1", "result": "done", "is_error": false, "hang": false}`, each step one of
//! `{"text": "…"}`, `{"tool": "write_file", "arguments": {…}}`, `{"builtin": "Read", "input":
//! {…}}`, `{"bash": "uv run pytest"}`, `{"sleep_ms": 100}`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    write_to_env_file("FAKE_LACKEY_ARGS", &args.join("\n"));
    write_to_env_file(
        "FAKE_LACKEY_TOKEN",
        &std::env::var("LACKEY_TOKEN").unwrap_or_default(),
    );
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut first = String::new();
    let _ = input.read_line(&mut first);
    write_to_env_file("FAKE_LACKEY_STDIN", &first);
    let script: Value = std::env::var("FAKE_LACKEY_SCRIPT")
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}));
    let session = script["session"]
        .as_str()
        .unwrap_or("fake-session")
        .to_string();
    emit(json!({ "type": "system", "subtype": "init", "session_id": session }));
    let mut mcp = flag(&args, "--mcp-config").and_then(|config| Mcp::start(&config));
    let hooks = flag(&args, "--settings")
        .and_then(|settings| serde_json::from_str::<Value>(&settings).ok())
        .unwrap_or_else(|| json!({}));
    let steps = script["steps"].as_array().cloned().unwrap_or_default();
    for step in &steps {
        if play(step, mcp.as_mut(), &hooks) == Go::Stop {
            break;
        }
    }
    emit(json!({
        "type": "result", "subtype": "success",
        "is_error": script["is_error"].as_bool().unwrap_or(false),
        "result": script["result"].as_str().unwrap_or(""),
        "total_cost_usd": script["cost"].as_f64().unwrap_or(0.01),
        "num_turns": steps.len(), "session_id": session,
    }));
    run_hook(&hooks, "Stop", json!({ "hook_event_name": "Stop" }));
    if script["hang"].as_bool() == Some(true) {
        std::thread::sleep(std::time::Duration::from_secs(600));
    }
    // the turn is over: wait for the input to close, as the real one does
    let mut rest = String::new();
    while input.read_line(&mut rest).map(|n| n > 0).unwrap_or(false) {
        rest.clear();
    }
}

#[derive(PartialEq)]
enum Go {
    On,
    Stop,
}

fn play(step: &Value, mcp: Option<&mut Mcp>, hooks: &Value) -> Go {
    if let Some(text) = step["text"].as_str() {
        emit(assistant(json!({ "type": "text", "text": text })));
    } else if let Some(name) = step["tool"].as_str() {
        let full = format!("mcp__lackey__{name}");
        emit(assistant(
            json!({ "type": "tool_use", "id": "u", "name": full, "input": step["arguments"] }),
        ));
        let result = mcp.map(|mcp| mcp.call(name, step["arguments"].clone()));
        log(json!({ "tool": name, "result": result }));
        return after_tool(hooks, &full);
    } else if let Some(name) = step["builtin"].as_str() {
        emit(assistant(
            json!({ "type": "tool_use", "id": "u", "name": name, "input": step["input"] }),
        ));
        return after_tool(hooks, name);
    } else if let Some(command) = step["bash"].as_str() {
        let input = json!({ "command": command });
        if asks_every_time(hooks, "Bash") || !counted_read_only(command) {
            let asked = mcp
                .map(|mcp| mcp.call("permission", json!({ "tool_name": "Bash", "input": input })));
            log(json!({ "permission": asked, "command": command }));
        } else {
            log(json!({ "ran_unasked": command }));
        }
        emit(assistant(
            json!({ "type": "tool_use", "id": "u", "name": "Bash", "input": input }),
        ));
        return after_tool(hooks, "Bash");
    } else if let Some(ms) = step["sleep_ms"].as_u64() {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    Go::On
}

/// Whether the settings ask about every call of the tool (`"permissions": {"ask": ["Bash"]}`).
fn asks_every_time(settings: &Value, tool: &str) -> bool {
    settings["permissions"]["ask"]
        .as_array()
        .is_some_and(|asked| asked.iter().any(|rule| rule == tool))
}

/// A few of the commands the real `claude` runs unasked as read-only.
fn counted_read_only(command: &str) -> bool {
    let program = command.split_whitespace().next().unwrap_or_default();
    ["cat", "head", "tail", "grep", "ls", "find", "git", "wc"].contains(&program)
}

/// Runs the `PostToolUse` hook; a hook that answers `continue: false` stops the steps.
fn after_tool(hooks: &Value, tool: &str) -> Go {
    let output = run_hook(
        hooks,
        "PostToolUse",
        json!({ "hook_event_name": "PostToolUse", "tool_name": tool }),
    );
    log(json!({ "hook": "PostToolUse", "after": tool, "output": output }));
    let stops = serde_json::from_str::<Value>(&output)
        .is_ok_and(|output| output["continue"] == json!(false));
    if stops { Go::Stop } else { Go::On }
}

fn assistant(block: Value) -> Value {
    json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "content": [block] } })
}

fn emit(message: Value) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}

fn log(entry: Value) {
    let Ok(path) = std::env::var("FAKE_LACKEY_LOG") else {
        return;
    };
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{entry}");
    }
}

fn write_to_env_file(var: &str, text: &str) {
    if let Ok(path) = std::env::var(var) {
        let _ = std::fs::write(path, text);
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at + 1).cloned()
}

/// Runs the first hook command for the event with the input on its stdin; its stdout.
fn run_hook(hooks: &Value, event: &str, input: Value) -> String {
    let Some(command) = hooks["hooks"][event][0]["hooks"][0]["command"].as_str() else {
        return String::new();
    };
    let words = words_of(command);
    let Some((program, args)) = words.split_first() else {
        return String::new();
    };
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        return String::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{input}");
    }
    child
        .wait_with_output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

/// A command line's words, split as a POSIX shell does for what `helper_command` writes:
/// single quotes hold anything, and a backslash outside them keeps the next character.
fn words_of(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => quoted = !quoted,
            '\\' if !quoted => word.extend(chars.next()),
            ' ' if !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// The MCP server of `--mcp-config`, started and initialized.
struct Mcp {
    _child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn start(config: &str) -> Option<Mcp> {
        let config: Value = serde_json::from_str(config).ok()?;
        let server = config["mcpServers"]["lackey"].clone();
        let mut command = Command::new(server["command"].as_str()?);
        let args: Vec<String> = server["args"]
            .as_array()?
            .iter()
            .filter_map(|arg| arg.as_str().map(String::from))
            .collect();
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        for (name, value) in server["env"].as_object().into_iter().flatten() {
            command.env(name, value.as_str().unwrap_or_default());
        }
        let mut child = command.spawn().ok()?;
        let mut mcp = Mcp {
            input: child.stdin.take()?,
            output: BufReader::new(child.stdout.take()?),
            _child: child,
            next_id: 0,
        };
        mcp.request(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
        )?;
        let _ = writeln!(
            mcp.input,
            "{}",
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
        );
        Some(mcp)
    }

    fn request(&mut self, method: &str, params: Value) -> Option<Value> {
        self.next_id += 1;
        let message =
            json!({ "jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params });
        writeln!(self.input, "{message}").ok()?;
        self.input.flush().ok()?;
        let mut line = String::new();
        self.output.read_line(&mut line).ok()?;
        serde_json::from_str::<Value>(&line)
            .ok()
            .map(|reply| reply["result"].clone())
    }

    /// A tool's result: `{"text": …, "is_error": …}`.
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        match self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        ) {
            Some(result) => json!({
                "text": result["content"][0]["text"],
                "is_error": result["isError"],
            }),
            None => json!({ "text": "no reply", "is_error": true }),
        }
    }
}
