//! Following a running task: what the agent does becomes the status line and the journal, its
//! final text is posted when it used no medium, and the end of its turn (or a stop) finishes it.

use std::time::Duration;

use crate::domain::agent_event::{AgentEvent, Finished};
use crate::domain::mediums::is_a_medium;
use crate::domain::status::{CommandVerdict, activity_of, command_activity, status_text};
use crate::domain::task::{TaskRecord, TaskState};
use crate::ports::agent::NextEvent;

use super::finish::{Ending, finish_task};
use super::start::StartedTask;
use super::task_deps::{LiveTask, TaskDeps};

/// How long one look at the agent waits before checking for a stop.
const LOOK: Duration = Duration::from_millis(200);

/// A final text this long or shorter, on one line, is a notification; longer is a note.
const SHORT_TEXT: usize = 200;

/// Follows the task until its turn ends, the agent ends, or the person stops it, then
/// finishes it (the record, the ledger, the done message) and gives the record back.
///
/// Example: an agent that reads `booking.py`, writes `cancel_booking.py` through `write_file`
/// and ends its turn at $0.71 moves the status line through `reading booking.py` and `writing
/// cancel_booking.py`, and finishes `done` at $0.71.
pub fn follow_task(started: StartedTask, deps: &TaskDeps) -> TaskRecord {
    let StartedTask { live, mut running } = started;
    deps.editor
        .progress(&live.id, &status_text(&live.title, "starting", 0.0));
    let ending = loop {
        if live.cancel.is_cancelled() {
            running.stop();
            break Ending::Stopped;
        }
        match running.next_event(LOOK) {
            NextEvent::Waiting => {}
            NextEvent::Ended => break Ending::EndedEarly,
            NextEvent::Event(AgentEvent::Finished(finished)) => {
                running.close_input();
                break Ending::Finished(finished);
            }
            NextEvent::Event(event) => saw(&live, &event, deps),
        }
    };
    if let Ending::Finished(finished) = &ending {
        post_what_it_said_unheard(&live, finished, deps);
    }
    finish_task(&live, ending, deps)
}

/// One thing the agent did: the session's id kept, a tool call shown on the status line and
/// journaled. A command is journaled by the permission tool, which every command goes through,
/// so the journal never says `running ls` for a command still waiting or denied.
fn saw(live: &LiveTask, event: &AgentEvent, deps: &TaskDeps) {
    match event {
        AgentEvent::Started { session_id } => {
            let record = live.update(|record| record.session_id = Some(session_id.clone()));
            let _ = deps.tasks.append(&record);
        }
        AgentEvent::ToolUse {
            name,
            input,
            parent,
        } => {
            if is_a_medium(name) {
                live.spoke();
            }
            let command = input["command"].as_str().unwrap_or_default();
            let verdict = live.take_command_verdict(command);
            // the question was asked before the stream showed the call: the status line already
            // says `waiting for you`, and the call must not overwrite it with `until allowed`
            let asked_first = verdict == Some(CommandVerdict::Waiting) && !command.is_empty();
            let activity = match verdict {
                Some(verdict) if !command.is_empty() => command_activity(command, verdict),
                _ => activity_of(name, input, parent.as_deref(), &deps.root),
            };
            if !asked_first {
                show_activity(live, &activity, deps);
            }
            // a command's lines are the permission tool's: waiting, then the answer
            if command.is_empty() {
                let _ = deps.journal.write(&live.id, "tool", &activity);
            }
        }
        AgentEvent::Used {
            message_id,
            model,
            usage,
        } => live.used(message_id, model, *usage),
        AgentEvent::Text { .. } | AgentEvent::Finished(_) => {}
    }
}

/// The status line: the task's title, what it does now, and what the stream has shown it cost.
pub(super) fn show_activity(live: &LiveTask, activity: &str, deps: &TaskDeps) {
    let spent = live.cost_seen(deps.price_of).unwrap_or(0.0);
    deps.editor
        .progress(&live.id, &status_text(&live.title, activity, spent));
}

/// When the agent ended its turn without a medium, its final text is posted: a notification
/// when short, a note when not, so nothing it decided to say is lost in a stream nobody reads.
fn post_what_it_said_unheard(live: &LiveTask, finished: &Finished, deps: &TaskDeps) {
    let text = finished.text.trim();
    if live.has_spoken() || text.is_empty() || finished.is_error {
        return;
    }
    if text.chars().count() <= SHORT_TEXT && !text.contains('\n') {
        deps.editor.notify(&format!("lackey: {text}"));
        let _ = deps.journal.write(&live.id, "notify (final text)", text);
    } else {
        let _ = deps.editor.note(&live.id, &live.title, text);
        let _ = deps.journal.write(&live.id, "note (final text)", text);
    }
}

/// The task's state once its turn ended.
pub(super) fn state_after(ending: &Ending) -> TaskState {
    match ending {
        Ending::Finished(finished) if !finished.is_error => TaskState::Done,
        Ending::Finished(_) | Ending::EndedEarly => TaskState::Failed,
        Ending::Stopped => TaskState::Stopped,
    }
}
