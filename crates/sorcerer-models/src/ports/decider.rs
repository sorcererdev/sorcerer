//! The decider seam: how sorcerer asks Jev (or a recorded double) typed
//! questions.

use sorcerer_core::Cancel;

use crate::Result;
use crate::domain::decide::{Question, Weighed};

pub trait Decider: Send + Sync {
    /// Asks a batch of questions in one call and returns one answer per
    /// question, in the same order. The questions share their context, so
    /// a text named in two of them is sent once.
    ///
    /// Example: the claim checker's three guards go as one batch and come
    /// back as three yes/no answers.
    fn ask(&self, questions: &[Question], cancel: &Cancel) -> Result<Vec<Weighed>>;
}
