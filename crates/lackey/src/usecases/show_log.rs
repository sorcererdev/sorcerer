//! The last tasks recorded, newest first, for `lackey log`.

use sorcerer_core::Result;

use crate::domain::task::TaskRecord;
use crate::ports::task_store::TaskStore;

/// The last `n` tasks as they stand now, the newest first.
pub fn last_tasks(store: &dyn TaskStore, n: usize) -> Result<Vec<TaskRecord>> {
    let rows = store.read()?;
    Ok(rows.tasks.into_iter().rev().take(n).collect())
}
