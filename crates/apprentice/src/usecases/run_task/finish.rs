//! Finishing a task: its record, its errand in the ledger, the done message and its marks.

use sorcerer_core::domain::errand::Errand;

use crate::domain::agent_event::Finished;
use crate::domain::config::PRODUCT;
use crate::domain::log_text::done_text;
use crate::domain::task::TaskRecord;

use super::follow::state_after;
use super::task_deps::{LiveTask, TaskDeps};

/// How a task's turn ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Ending {
    /// The agent reported the end of its turn.
    Finished(Finished),
    /// The agent ended without reporting one.
    EndedEarly,
    /// The person stopped it.
    Stopped,
}

/// The ledger's verb for a task.
pub const TASK_VERB: &str = "task";

/// Records the task's end, its errand and tells the editor.
///
/// 1. The record: the state (`done`, `failed`, `stopped`), the cost, the turns, the end time.
///    A task that reported no cost (stopped, ended early, or a result without one) is charged its whole cap, as every
///    call the suite cannot see the end of is.
/// 2. The ledger: one errand, product `apprentice`, verb `task`.
/// 3. The editor: the done message and the marks on the files it changed.
pub fn finish_task(live: &LiveTask, ending: Ending, deps: &TaskDeps) -> TaskRecord {
    let now = deps.clock.now();
    let cap = deps.config.budget_per_task;
    let record = live.update(|record| {
        record.state = state_after(&ending);
        record.ended = Some(now);
        match &ending {
            Ending::Finished(finished) => {
                record.cost_usd = finished.cost_usd.unwrap_or(cap);
                record.turns = finished.turns;
                record.session_id = finished.session_id.clone().or(record.session_id.take());
                if finished.is_error {
                    record.note = Some(first_line(&finished.text));
                }
            }
            Ending::EndedEarly => {
                record.cost_usd = cap;
                record.note = Some("the agent ended without finishing its turn".into());
            }
            Ending::Stopped => record.cost_usd = cap,
        }
    });
    let _ = deps.tasks.append(&record);
    let _ = deps.ledger.append(&errand_of(&record, &deps.config.model));
    let done = done_text(&record, record.seconds(now));
    let _ = deps.journal.write(&record.id, "end", &done);
    deps.editor.finished(&record, &done);
    record
}

fn errand_of(record: &TaskRecord, model: &str) -> Errand {
    Errand {
        at: record.ended.unwrap_or(record.started),
        product: PRODUCT.into(),
        verb: TASK_VERB.into(),
        model: model.into(),
        provider: "claude".into(),
        cost_usd: Some(record.cost_usd),
        input_tokens: 0,
        output_tokens: 0,
        seconds: record.seconds(record.ended.unwrap_or(record.started)),
        ok: record.state == crate::domain::task::TaskState::Done,
        note: record.note.clone(),
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect()
}
