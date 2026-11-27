//! The person's edits, fed to the agent after every tool call (the `PostToolUse` hook), and
//! journaled at the end of its turn (the `Stop` hook).

use crate::domain::edits_text::{diff_text, edits_context_text};
use crate::domain::loopback_message::LoopbackReply;

use super::task_deps::{LiveTask, TaskDeps};

/// The hook event at the end of a turn.
pub const STOP_EVENT: &str = "stop";

/// The person's edits since the task last looked, as the hook's context for the agent; empty
/// when they changed nothing. At the end of the turn (`stop`) the edits are journaled only:
/// handing them over there would mean refusing the stop, and a task that should go on is a new
/// instruction, not a refused stop.
///
/// Example: the person renamed `TooLate` to `TooLateToCancel` in `cancel_booking.py` while the
/// agent wrote the test: after its next tool call the agent reads that diff under "their edits
/// are authoritative".
pub fn edits_for_agent(live: &LiveTask, event: &str, deps: &TaskDeps) -> LoopbackReply {
    let edits = deps.editor.edits_since_look(&live.id);
    let diffs: String = edits.iter().map(diff_text).collect();
    if !diffs.is_empty() {
        let heading = if event == STOP_EVENT {
            "edits after the turn"
        } else {
            "edits fed"
        };
        let _ = deps.journal.write(&live.id, heading, &diffs);
    }
    if event == STOP_EVENT {
        return LoopbackReply::text("");
    }
    LoopbackReply::text(edits_context_text(&edits).unwrap_or_default())
}
