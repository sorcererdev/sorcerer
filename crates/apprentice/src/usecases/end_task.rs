//! A finished task's end: the person kept it (saved every file it changed) or took it back.

use crate::domain::task::{TaskRecord, TaskState};
use crate::ports::clock::Clock;
use crate::ports::task_store::TaskStore;

/// Records the task's last state, `kept` or `undone`, and gives the record back.
pub fn record_task_end(
    task: &TaskRecord,
    state: TaskState,
    tasks: &dyn TaskStore,
    clock: &dyn Clock,
) -> TaskRecord {
    let ended = TaskRecord {
        state,
        ended: task.ended.or(Some(clock.now())),
        ..task.clone()
    };
    let _ = tasks.append(&ended);
    ended
}
