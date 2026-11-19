//! An IdGenerator in memory: ids b1, b2, …, predictable, for tests and demos.

use std::cell::Cell;

use crate::booking::ports::id_generator::IdGenerator;

#[derive(Default)]
pub struct SequentialIds {
    n: Cell<u32>,
}

impl IdGenerator for SequentialIds {
    fn next_id(&self) -> String {
        self.n.set(self.n.get() + 1);
        format!("b{}", self.n.get())
    }
}
