//! Ids for real use: the clock's nanoseconds and a counter, in hex.

use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::booking::ports::id_generator::IdGenerator;

#[derive(Default)]
pub struct TimeIds {
    n: Cell<u32>,
}

impl IdGenerator for TimeIds {
    fn next_id(&self) -> String {
        self.n.set(self.n.get().wrapping_add(1));
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or_default();
        format!("{:08x}", nanos ^ self.n.get().rotate_left(16))
    }
}
