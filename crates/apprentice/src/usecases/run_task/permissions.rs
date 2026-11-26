//! The agent's permission questions: before a command (`Bash`) or any tool nothing allows,
//! `claude` asks the permission tool, which asks the person once. A command allowed *always for
//! this project* runs unasked from then on; the writers are refused outright, because every
//! change goes through `write_file`.

use std::time::Duration;

use serde_json::{Value, json};

use crate::domain::loopback_message::LoopbackReply;
use crate::domain::mediums::{Ask, AskWay};
use crate::domain::permissions::prefix_of;
use crate::ports::editor::Asked;

use super::task_deps::{LiveTask, TaskDeps};

const ALLOW_ONCE: &str = "Allow once";
const DENY: &str = "Deny";

/// The tools that write files, which no task may use.
const WRITERS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// Answers `claude`'s permission question about one tool call, as the permission tool's one
/// text block: `{"behavior":"allow","updatedInput":…}` or `{"behavior":"deny","message":…}`.
///
/// 1. A writer is denied: "use write_file". `AskUserQuestion` is denied: "use ask".
/// 2. A command allowed always runs.
/// 3. Anything else is asked through the picker: *Allow once*, *Always for this project
///    (<prefix>)* for a command that has one, *Deny*. No answer within the wait is a deny.
///
/// Example: `uv run pytest -q` asked, answered *Always for this project (uv run pytest)*, runs;
/// the next `uv run pytest tests` runs unasked.
pub fn answer_permission(
    live: &LiveTask,
    tool: &str,
    input: &Value,
    deps: &TaskDeps,
) -> LoopbackReply {
    if WRITERS.contains(&tool) {
        return deny("use write_file: it is the only way to change a file here");
    }
    if tool == "AskUserQuestion" {
        return deny("use the ask tool: the person answers in their editor");
    }
    let command = input["command"].as_str().unwrap_or_default();
    if tool == "Bash" && deps.permissions.allowed().allows(command) {
        let _ = deps.journal.write(&live.id, "allowed always", command);
        return allow(input);
    }
    ask_for(live, tool, command, input, deps)
}

fn ask_for(
    live: &LiveTask,
    tool: &str,
    command: &str,
    input: &Value,
    deps: &TaskDeps,
) -> LoopbackReply {
    let prefix = (tool == "Bash").then(|| prefix_of(command)).flatten();
    let always = prefix.as_ref().map(|prefix| always_text(prefix));
    let options: Vec<String> = [
        Some(ALLOW_ONCE.to_string()),
        always.clone(),
        Some(DENY.into()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let ask = Ask {
        question: question_text(tool, command),
        options,
        how: AskWay::Picker,
        at: None,
        default: Some(DENY.into()),
    };
    let wait = Duration::from_secs(deps.config.ask_timeout_seconds);
    let answer = deps.editor.ask(&live.id, &ask, wait);
    let entry = format!("{}\n→ {answer:?}", ask.question);
    let _ = deps.journal.write(&live.id, "permission", &entry);
    match answer {
        Asked::Answered(picked) if picked == ALLOW_ONCE => allow(input),
        Asked::Answered(picked) if Some(&picked) == always.as_ref() => {
            if let Some(prefix) = prefix {
                let _ = deps.permissions.allow_always(&prefix);
            }
            allow(input)
        }
        Asked::Answered(_) => deny("the person did not allow it; carry on without it"),
        Asked::NoAnswer => deny("no answer yet; carry on without it"),
    }
}

/// The picker's question: `apprentice wants to run: uv run pytest -q`.
fn question_text(tool: &str, command: &str) -> String {
    if command.is_empty() {
        format!("apprentice wants to use {tool}")
    } else {
        format!("apprentice wants to run: {command}")
    }
}

fn always_text(prefix: &str) -> String {
    format!("Always for this project ({prefix})")
}

fn allow(input: &Value) -> LoopbackReply {
    LoopbackReply::text(json!({ "behavior": "allow", "updatedInput": input }).to_string())
}

fn deny(message: &str) -> LoopbackReply {
    LoopbackReply::text(json!({ "behavior": "deny", "message": message }).to_string())
}
