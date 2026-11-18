//! A cancellation flag shared between a caller and long-running work.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cloned into every piece of work that should stop when the caller does.
#[derive(Clone, Default, Debug)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
