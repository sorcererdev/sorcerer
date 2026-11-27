//! A medium tool's call, answered: the six tools turned into what the editor shows, and the
//! sentence the agent gets back.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use sorcerer_core::domain::paths::forward_slashes;

use crate::domain::loopback_message::LoopbackReply;
use crate::domain::mediums::{Ask, ToolCall, tool_call_of};
use crate::domain::paths::{is_never_written, project_relative};
use crate::domain::task::TaskState;
use crate::ports::editor::{Asked, Written};

use super::task_deps::{LiveTask, TaskDeps};

/// Answers one call of a medium tool (named without its prefix): what the agent's tool result
/// says. A call with arguments missing, or a path outside what the task may write, is an error
/// result naming why, so the agent can try again.
///
/// Example: `write_file` of `src/cancel_booking.py` that lands answers `written:
/// src/cancel_booking.py (in the editor, unsaved)`; one whose file the person edited since the
/// agent read it answers an error with the file's text now.
pub fn answer_tool_call(
    live: &LiveTask,
    name: &str,
    arguments: &Value,
    deps: &TaskDeps,
) -> LoopbackReply {
    let call = match tool_call_of(name, arguments) {
        Ok(call) => call,
        Err(why) => return LoopbackReply::error(why),
    };
    live.spoke();
    match call {
        ToolCall::Notify { text } => {
            deps.editor.notify(&format!("apprentice: {text}"));
            journaled(live, deps, "notify", &text, LoopbackReply::text("shown"))
        }
        ToolCall::Mark {
            file,
            line,
            text,
            severity,
        } => match readable(&file, deps) {
            Ok(path) => {
                deps.editor.mark(&live.id, &path, line, &text, severity);
                let entry = format!("{file}:{line} {text}");
                journaled(live, deps, "mark", &entry, LoopbackReply::text("marked"))
            }
            Err(why) => LoopbackReply::error(why),
        },
        ToolCall::Ghost { file, line, lines } => match readable(&file, deps) {
            Ok(path) => {
                deps.editor.ghost(&path, line, &lines);
                let entry = format!("{file}:{line}\n{}", lines.join("\n"));
                journaled(live, deps, "ghost", &entry, LoopbackReply::text("shown"))
            }
            Err(why) => LoopbackReply::error(why),
        },
        ToolCall::WriteFile { path, text } => write_file(live, &path, &text, deps),
        ToolCall::Note { title, text } => match deps.editor.note(&live.id, &title, &text) {
            Ok(path) => {
                let reply = LoopbackReply::text(format!("note written: {}", path.display()));
                journaled(live, deps, "note", &format!("{title}\n\n{text}"), reply)
            }
            Err(why) => LoopbackReply::error(format!("the note was not written: {why}")),
        },
        ToolCall::Ask(ask) => ask_the_person(live, &ask, deps),
    }
}

/// A `write_file`: the path checked, then landed in the editor. The first landing of a file
/// keeps its text before, for undo and the diff view.
fn write_file(live: &LiveTask, path: &str, text: &str, deps: &TaskDeps) -> LoopbackReply {
    let relative = match writable(path, deps) {
        Ok(relative) => relative,
        Err(why) => return LoopbackReply::error(why),
    };
    let shown = forward_slashes(&relative);
    match deps.editor.write_file(&live.id, &relative, text) {
        Written::Landed { before } => {
            let _ = deps
                .journal
                .keep_before(&live.id, &shown, before.as_deref());
            // the first landing decides: a file the task created stays new
            live.changed_file(&shown, before.is_none());
            let reply = LoopbackReply::text(format!("written: {shown} (in the editor, unsaved)"));
            journaled(live, deps, "write_file", &shown, reply)
        }
        Written::MovedOn { now } => {
            let why = format!(
                "refused: {shown} changed since you read it (the person's edits win). Its text now:\n{now}"
            );
            journaled(
                live,
                deps,
                "write_file refused",
                &shown,
                LoopbackReply::error(why),
            )
        }
        Written::Refused(why) => {
            let why = format!("refused: {shown}: {why}");
            journaled(
                live,
                deps,
                "write_file refused",
                &why,
                LoopbackReply::error(why.clone()),
            )
        }
    }
}

/// A question: the task waits, `asking`, for the answer or the timeout.
fn ask_the_person(live: &LiveTask, ask: &Ask, deps: &TaskDeps) -> LoopbackReply {
    let asking = live.set_state(TaskState::Asking);
    let _ = deps.tasks.append(&asking);
    let wait = Duration::from_secs(deps.config.ask_timeout_seconds);
    let asked = deps.editor.ask(&live.id, ask, wait);
    if live.is_live() {
        let running = live.set_state(TaskState::Running);
        let _ = deps.tasks.append(&running);
    }
    let (entry, reply) = match asked {
        Asked::Answered(answer) => (
            format!("{}\n→ {answer}", ask.question),
            LoopbackReply::text(format!("the person chose: {answer}")),
        ),
        Asked::NoAnswer => (
            format!("{}\n→ no answer", ask.question),
            LoopbackReply::text(no_answer_text(ask)),
        ),
    };
    journaled(live, deps, "ask", &entry, reply)
}

fn no_answer_text(ask: &Ask) -> String {
    match &ask.default {
        Some(default) => format!("no answer yet; carry on with the default ({default})"),
        None => "no answer yet; carry on with the default".to_string(),
    }
}

/// A path the agent may name in a mark or a ghost: inside the project and not fenced.
fn readable(written: &str, deps: &TaskDeps) -> Result<PathBuf, String> {
    let relative = project_relative(&deps.root, written)
        .ok_or_else(|| format!("{written} is not a path inside the project"))?;
    if !deps.fences.allows(&deps.root.join(&relative)) {
        return Err(format!("{written} is fenced: the task may not touch it"));
    }
    Ok(relative)
}

/// A path the agent may write: readable, and not one `library/never-written.toml` names
/// (`.sorcerer/`, `.git/`, an agent's or a shell's config).
fn writable(written: &str, deps: &TaskDeps) -> Result<PathBuf, String> {
    let relative = readable(written, deps)?;
    if is_never_written(&relative) {
        return Err(format!(
            "{written} is config a tool obeys or sorcerer's own: no task writes there"
        ));
    }
    Ok(relative)
}

/// Journals the entry and gives the reply back.
fn journaled(
    live: &LiveTask,
    deps: &TaskDeps,
    heading: &str,
    entry: &str,
    reply: LoopbackReply,
) -> LoopbackReply {
    let _ = deps.journal.write(&live.id, heading, entry);
    reply
}
