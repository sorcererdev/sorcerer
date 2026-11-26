//! What a task runs with, and a running task's shared state: the follow loop and the helper
//! processes' requests (each on a thread of its own) meet here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sorcerer_core::Cancel;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::ports::ledger::Ledger;

use crate::domain::config::ApprenticeConfig;
use crate::domain::log_text::title_of;
use crate::domain::task::{TaskFile, TaskRecord, TaskState};
use crate::ports::agent::Agent;
use crate::ports::clock::Clock;
use crate::ports::editor::Editor;
use crate::ports::journal::Journal;
use crate::ports::permission_store::PermissionStore;
use crate::ports::project_context::ProjectContext;
use crate::ports::task_store::TaskStore;

/// Everything a task needs besides its order. Shared (each field is cheap to clone) because a
/// task's helper processes call in from threads of their own.
#[derive(Clone)]
pub struct TaskDeps {
    pub agent: Arc<dyn Agent>,
    pub editor: Arc<dyn Editor>,
    pub tasks: Arc<dyn TaskStore>,
    pub journal: Arc<dyn Journal>,
    pub permissions: Arc<dyn PermissionStore>,
    pub ledger: Arc<dyn Ledger>,
    pub clock: Arc<dyn Clock>,
    pub context: Arc<dyn ProjectContext>,
    /// What the agent may not read or write.
    pub fences: Fences,
    pub root: PathBuf,
    pub config: ApprenticeConfig,
}

/// A task while it runs.
pub struct LiveTask {
    pub id: String,
    /// The instruction's first sentence, for the status line and the marks.
    pub title: String,
    record: Mutex<TaskRecord>,
    /// Whether the agent spoke through a medium in this turn: if it did not, its final text is
    /// posted so nothing it said is lost.
    spoke: AtomicBool,
    pub cancel: Cancel,
}

impl LiveTask {
    pub fn new(record: TaskRecord) -> LiveTask {
        LiveTask {
            id: record.id.clone(),
            title: title_of(&record.instruction),
            record: Mutex::new(record),
            spoke: AtomicBool::new(false),
            cancel: Cancel::default(),
        }
    }

    /// The task as it stands.
    pub fn record(&self) -> TaskRecord {
        lock(&self.record).clone()
    }

    /// Changes the record and gives the new one back.
    pub fn update(&self, change: impl FnOnce(&mut TaskRecord)) -> TaskRecord {
        let mut record = lock(&self.record);
        change(&mut record);
        record.clone()
    }

    /// Counts a file as changed by the task (once).
    pub fn changed_file(&self, path: &str, new: bool) {
        self.update(|record| {
            if !record.files.iter().any(|file| file.path == path) {
                record.files.push(TaskFile {
                    path: path.to_string(),
                    new,
                });
            }
        });
    }

    pub fn spoke(&self) {
        self.spoke.store(true, Ordering::SeqCst);
    }

    pub fn has_spoken(&self) -> bool {
        self.spoke.load(Ordering::SeqCst)
    }

    /// Whether it is still running or asking.
    pub fn is_live(&self) -> bool {
        !lock(&self.record).state.is_over()
    }

    pub fn set_state(&self, state: TaskState) -> TaskRecord {
        self.update(|record| record.state = state)
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
