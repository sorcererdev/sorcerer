//! The decider seam.

use sorcerer_core::Cancel;

use crate::Result;
use crate::domain::decide::{Question, Weighed};

pub trait Decider: Send + Sync {
    /// One call for the batch, one shared state; one answer a question, in order.
    fn ask(&self, qs: &[Question], cancel: &Cancel) -> Result<Vec<Weighed>>;
}
