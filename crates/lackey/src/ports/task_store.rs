//! Where tasks are recorded (`tasks.jsonl`).

use sorcerer_core::Result;

use crate::domain::task::{TaskRecord, TaskRows};

pub trait TaskStore: Send + Sync {
    /// Records a task as it stands now; the last record of an id is the task.
    fn append(&self, task: &TaskRecord) -> Result<()>;
    /// The last record of each task, oldest first.
    fn read(&self) -> Result<TaskRows>;
}
