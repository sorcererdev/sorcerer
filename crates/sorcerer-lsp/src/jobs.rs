//! Work to run after a delay, one waiting job per key: a later submit under the same key
//! replaces the waiting one and starts the delay again.
//!
//! This is how a burst of saves becomes one pass: each save submits under `"pass"`, and only
//! the last one's job runs, a delay after the last save.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Delayed jobs by key. Each job waits and runs on a thread of its own, never the caller's.
/// Dropping `Jobs` drops what is still waiting: those jobs never run.
pub struct Jobs {
    waiting: Arc<Mutex<Waiting>>,
}

/// Which submit is the live one for each key; a job runs only if it is still the live one
/// when its delay ends.
#[derive(Default)]
struct Waiting {
    live: HashMap<String, u64>,
    submitted: u64,
    dropped: bool,
}

impl Default for Jobs {
    fn default() -> Jobs {
        Jobs::new()
    }
}

impl Jobs {
    /// No job waiting.
    pub fn new() -> Jobs {
        Jobs {
            waiting: Arc::new(Mutex::new(Waiting::default())),
        }
    }

    /// Runs `job` after `delay`, on its own thread, unless another submit under the same key
    /// or a `cancel` comes first.
    ///
    /// Example: submitting under `"pass"` at 0 ms, 10 ms and 20 ms with a 30 ms delay runs
    /// only the third job, at about 50 ms.
    pub fn submit(&self, key: &str, delay: Duration, job: impl FnOnce() + Send + 'static) {
        let ticket = self.take_ticket(key);
        let waiting = Arc::clone(&self.waiting);
        let key = key.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            if still_live(&waiting, &key, ticket) {
                job();
            }
        });
    }

    /// Drops the job waiting under a key, if there is one. A job already running is not
    /// stopped.
    pub fn cancel(&self, key: &str) {
        lock(&self.waiting).live.remove(key);
    }

    /// Makes a new submit the live one for its key.
    fn take_ticket(&self, key: &str) -> u64 {
        let mut waiting = lock(&self.waiting);
        waiting.submitted += 1;
        let ticket = waiting.submitted;
        waiting.live.insert(key.to_string(), ticket);
        ticket
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        lock(&self.waiting).dropped = true;
    }
}

/// Whether the job holding this ticket is still the one to run; if so, it is taken off the
/// waiting list as it starts.
fn still_live(waiting: &Mutex<Waiting>, key: &str, ticket: u64) -> bool {
    let mut waiting = lock(waiting);
    let live = !waiting.dropped && waiting.live.get(key) == Some(&ticket);
    if live {
        waiting.live.remove(key);
    }
    live
}

/// The waiting list, also after a job panicked while holding it.
fn lock(waiting: &Mutex<Waiting>) -> std::sync::MutexGuard<'_, Waiting> {
    waiting
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
