//! Running tasks with their way back: the loopback port open, each live task findable by its
//! id, so a helper process's request reaches the task it names.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::domain::loopback_message::LoopbackReply;
use crate::domain::task::TaskRecord;
use crate::usecases::run_task::{
    LiveTask, StartedTask, TaskDeps, TaskOrder, TaskRefused, answer_request, follow_task,
    start_task,
};

use super::loopback::{Handler, Loopback};

/// The tasks a server (or one command) runs, and the port their helpers call back on.
pub struct TaskRunner {
    deps: TaskDeps,
    live: Arc<Mutex<HashMap<String, Arc<LiveTask>>>>,
    loopback: Loopback,
}

impl TaskRunner {
    /// Opens the loopback port; each request is answered for the live task it names.
    pub fn open(deps: TaskDeps) -> std::io::Result<TaskRunner> {
        let live: Arc<Mutex<HashMap<String, Arc<LiveTask>>>> = Arc::default();
        let (known, answering) = (live.clone(), deps.clone());
        let handler: Handler = Arc::new(move |request| {
            let task = lock(&known).get(&request.task).cloned();
            match task {
                Some(task) => answer_request(&task, &request.body, &answering),
                None => LoopbackReply::error(format!("no task {}", request.task)),
            }
        });
        Ok(TaskRunner {
            deps,
            live,
            loopback: Loopback::open(handler)?,
        })
    }

    /// Starts a task and makes it findable by its helpers.
    pub fn start(&self, order: &TaskOrder) -> Result<StartedTask, TaskRefused> {
        let started = start_task(order, self.loopback.way_back(), &self.deps)?;
        lock(&self.live).insert(started.live.id.clone(), started.live.clone());
        Ok(started)
    }

    /// Follows a started task to its end (on the calling thread) and forgets it.
    pub fn follow(&self, started: StartedTask) -> TaskRecord {
        let id = started.live.id.clone();
        let record = follow_task(started, &self.deps);
        lock(&self.live).remove(&id);
        record
    }

    /// Starts and follows a task.
    pub fn run(&self, order: &TaskOrder) -> Result<TaskRecord, TaskRefused> {
        let started = self.start(order)?;
        Ok(self.follow(started))
    }

    /// A live task by its id.
    pub fn live(&self, id: &str) -> Option<Arc<LiveTask>> {
        lock(&self.live).get(id).cloned()
    }

    /// Every live task.
    pub fn all_live(&self) -> Vec<Arc<LiveTask>> {
        lock(&self.live).values().cloned().collect()
    }

    pub fn deps(&self) -> &TaskDeps {
        &self.deps
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
