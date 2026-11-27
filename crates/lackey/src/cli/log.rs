//! `lackey log`: the last tasks, newest first.

use std::io::Write;

use anyhow::Result;

use crate::domain::log_text::task_line;
use crate::ports::task_store::TaskStore;
use crate::usecases::show_log::last_tasks;

/// How many tasks `lackey log` shows when not asked for a number.
pub(super) const DEFAULT_COUNT: usize = 10;

/// Prints the last `n` tasks, one line each. None yet is said on stderr and exits 0.
pub(super) fn show_log(
    store: &dyn TaskStore,
    n: usize,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let tasks = last_tasks(store, n)?;
    if tasks.is_empty() {
        writeln!(err, "lackey: no tasks recorded yet")?;
        return Ok(0);
    }
    for task in &tasks {
        writeln!(out, "{}", task_line(task))?;
    }
    Ok(0)
}
