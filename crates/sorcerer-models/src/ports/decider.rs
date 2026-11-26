//! The decider seam: how sorcerer asks Jev (or a recorded double) typed
//! questions.

use sorcerer_core::Cancel;

use crate::Result;
use crate::domain::decide::{Question, Weighed};

pub trait Decider: Send + Sync {
    /// Asks a batch of questions and returns one answer per question, in
    /// the same order. Any batch is allowed: questions of one kind, or with
    /// different texts under one context name, are the decider's to split
    /// into as many calls as it needs. Questions that can share a call do,
    /// so a text named in two of them is sent once.
    ///
    /// Example: the claim checker's three guards go as one call and come
    /// back as three yes/no answers; scry's two "same problem?" questions
    /// about two stored findings go as two calls.
    fn ask(&self, questions: &[Question], cancel: &Cancel) -> Result<Vec<Weighed>>;
}
