//! The agent's permission questions: before every command (`Bash`, `PowerShell`, `Monitor`, which
//! `claude` is told to ask about) or any tool nothing allows, `claude` asks the permission tool,
//! which asks the person once. A command line allowed *always for this project* runs unasked
//! from then on (that exact line, never another use of the program); the writers are refused outright, because every change goes through `write_file`,
//! and so is a command that names a fenced path, whatever the person allowed.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::domain::loopback_message::LoopbackReply;
use crate::domain::mediums::{Ask, AskWay};
use crate::domain::permissions::{always_for, words_naming_paths};
use crate::domain::status::{CommandVerdict, command_activity};
use crate::domain::tree_readers::TreeReaders;
use crate::ports::editor::Asked;

use super::follow::show_activity;
use super::task_deps::{LiveTask, TaskDeps};
use sorcerer_core::domain::fences::Fences;

const ALLOW_ONCE: &str = "Allow once";
const DENY: &str = "Deny";

/// The tools that write files, which no task may use.
const WRITERS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// Answers `claude`'s permission question about one tool call, as the permission tool's one
/// text block: `{"behavior":"allow","updatedInput":…}` or `{"behavior":"deny","message":…}`.
///
/// 1. A writer is denied: "use write_file". `AskUserQuestion` is denied: "use ask".
/// 2. A command that names a fenced path is denied, even one allowed always: `git show
///    HEAD:.env` or `ls .sorcerer` never runs. (A command that reads a fenced file without
///    naming it, such as `grep -r KEY .`, is caught by the person, who is asked.)
/// 3. A command line allowed always runs.
/// 4. Anything else is asked through the picker: *Allow once*, *Always for this project
///    (<the command line>)* unless it chains commands, reads a whole tree or has a glob, and
///    *Deny*; an editor with no picker gets the same choices on the instruction's line. No
///    answer within the wait is a deny.
///
/// Example: `uv run pytest -q` asked, answered *Always for this project (uv run pytest -q)*,
/// runs; the next `uv run pytest -q` runs unasked, `uv run pytest tests` is asked about.
pub fn answer_permission(
    live: &LiveTask,
    tool: &str,
    input: &Value,
    deps: &TaskDeps,
) -> LoopbackReply {
    let command = input["command"].as_str().unwrap_or_default();
    let (verdict, reply) = permission_reply(live, tool, input, deps);
    say_where_the_command_stands(live, command, verdict, deps);
    reply
}

/// The verdict on a tool call, and the permission tool's reply that says it.
type Verdict = (CommandVerdict, LoopbackReply);

/// The status line and the journal say where a command stands with the person (`waiting for
/// you: ls`, then `running ls` or `denied: ls`), and the verdict is kept for when the stream
/// shows the command (it may come after the answer).
fn say_where_the_command_stands(
    live: &LiveTask,
    command: &str,
    verdict: CommandVerdict,
    deps: &TaskDeps,
) {
    if command.is_empty() {
        return;
    }
    live.command_verdict(command, verdict);
    let activity = command_activity(command, verdict);
    show_activity(live, &activity, deps);
    let _ = deps.journal.write(&live.id, "tool", &activity);
}

fn permission_reply(live: &LiveTask, tool: &str, input: &Value, deps: &TaskDeps) -> Verdict {
    if WRITERS.contains(&tool) {
        return deny("use write_file: it is the only way to change a file here");
    }
    if tool == "AskUserQuestion" {
        return deny("use the ask tool: the person answers in their editor");
    }
    let command = input["command"].as_str().unwrap_or_default();
    if let Some(fenced) = fenced_path_named(command, &deps.fences) {
        let _ = deps.journal.write(&live.id, "refused, fenced", command);
        return deny(&format!(
            "{fenced} is fenced: no task may read it, and no command may name it"
        ));
    }
    if tool == "Bash"
        && deps
            .permissions
            .allowed()
            .allows(command, TreeReaders::shipped())
    {
        let _ = deps.journal.write(&live.id, "allowed always", command);
        return allow(input);
    }
    ask_for(live, tool, command, input, deps)
}

/// The first word of the command that names a path inside the project the fences keep out: a
/// fenced file, or a directory everything in which is fenced (`.git`, `.sorcerer`). The project
/// root itself (`.`) is not one, and a word naming a path outside the project is left to the
/// person: commands name programs and system files all the time.
fn fenced_path_named<'a>(command: &'a str, fences: &Fences) -> Option<&'a str> {
    words_naming_paths(command).find(|word| {
        fences
            .relative(Path::new(word))
            .is_some_and(|relative| is_fenced(&relative, fences))
    })
}

fn is_fenced(relative: &Path, fences: &Fences) -> bool {
    let something_inside = relative.join("anything");
    !relative.as_os_str().is_empty()
        && !(fences.allows_relative(relative) && fences.allows_relative(&something_inside))
}

fn ask_for(live: &LiveTask, tool: &str, command: &str, input: &Value, deps: &TaskDeps) -> Verdict {
    let line = (tool == "Bash")
        .then(|| always_for(command, TreeReaders::shipped()))
        .flatten();
    let always = line.as_ref().map(|line| always_text(line));
    let options: Vec<String> = [
        Some(ALLOW_ONCE.to_string()),
        always.clone(),
        Some(DENY.into()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let task = live.record();
    let ask = Ask {
        question: question_text(tool, command),
        options,
        how: AskWay::Picker,
        // an editor with no picker gets the choices on the instruction's line instead
        at: Some((task.file, task.line)),
        default: Some(DENY.into()),
    };
    let wait = Duration::from_secs(deps.config.ask_timeout_seconds);
    say_where_the_command_stands(live, command, CommandVerdict::Waiting, deps);
    let answer = deps.editor.ask(&live.id, &ask, wait);
    let entry = format!("{}\n→ {answer:?}", ask.question);
    let _ = deps.journal.write(&live.id, "permission", &entry);
    match answer {
        Asked::Answered(picked) if picked == ALLOW_ONCE => allow(input),
        Asked::Answered(picked) if Some(&picked) == always.as_ref() => {
            if let Some(line) = line {
                let _ = deps.permissions.allow_always(&line);
            }
            allow(input)
        }
        Asked::Answered(_) => deny("the person did not allow it; carry on without it"),
        Asked::NoAnswer => deny("no answer yet; carry on without it"),
    }
}

/// The picker's question: `lackey wants to run: uv run pytest -q`.
fn question_text(tool: &str, command: &str) -> String {
    if command.is_empty() {
        format!("lackey wants to use {tool}")
    } else {
        format!("lackey wants to run: {command}")
    }
}

fn always_text(line: &str) -> String {
    format!("Always for this project ({line})")
}

fn allow(input: &Value) -> Verdict {
    let reply = json!({ "behavior": "allow", "updatedInput": input });
    (
        CommandVerdict::Allowed,
        LoopbackReply::text(reply.to_string()),
    )
}

fn deny(message: &str) -> Verdict {
    let reply = json!({ "behavior": "deny", "message": message });
    (
        CommandVerdict::Denied,
        LoopbackReply::text(reply.to_string()),
    )
}
