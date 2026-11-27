//! A task's journal (`tasks/<id>/journal.md`): every message the agent sent and through which
//! medium, every question and its answer, every refused write, the person's diffs it was fed;
//! and each changed file's text before the task (`tasks/<id>/before/<path>`), for undo and the
//! diff view.

use std::path::PathBuf;

use sorcerer_core::Result;

pub trait Journal: Send + Sync {
    /// Appends one entry under a heading (`notify`, `edits fed`, …).
    fn write(&self, task: &str, heading: &str, text: &str) -> Result<()>;
    /// Keeps a file's text before the task changed it, once: a second call for the same file
    /// keeps the first. `None` for a file the task created.
    fn keep_before(&self, task: &str, path: &str, before: Option<&str>) -> Result<()>;
    /// A file's text before the task: `Some(None)` for a file it created, `None` when the task
    /// never changed it.
    fn before(&self, task: &str, path: &str) -> Option<Option<String>>;
    /// Where the journal is, to show it.
    fn path(&self, task: &str) -> PathBuf;
}
