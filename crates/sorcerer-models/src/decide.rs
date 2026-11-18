//! The decider: Jev, or anything that answers typed questions (yes or no,
//! a choice, a level) with calibrated probabilities. It never writes text.
//! A calibrated yes/no costs a fraction of a cent, answers in under a
//! second, and gives a probability a cut can be put on.

pub mod jev;
pub mod recorded;

use sorcerer_core::Cancel;

use crate::Result;

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// One of these options (name, description).
    Choice(Vec<(String, String)>),
    /// Yes or no; `yes` and `no` describe the two sides.
    Noul { yes: String, no: String },
    /// A level, one description per level from 0.
    Score(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// The question's name in its batch (`nitpick`, `same-problem-3`).
    pub kind: String,
    pub instructions: String,
    pub shape: Shape,
    /// Named texts the question is about. A name shared by two questions
    /// of one batch must have one value.
    pub context: Vec<(String, String)>,
    /// A choice may be answered "none of these fits".
    pub unsure_allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Confidence {
    Known(f64),
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Chosen(String, Confidence),
    Yes(Confidence),
    No(Confidence),
    /// The level, its score, and the confidence.
    Level(u8, f64, Confidence),
    Unsure,
}

impl Answer {
    /// P(yes) of a yes/no answer; None otherwise.
    pub fn p_yes(&self) -> Option<f64> {
        match self {
            Answer::Yes(Confidence::Known(p)) => Some(*p),
            Answer::No(Confidence::Known(p)) => Some(1.0 - p),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Weighed {
    pub answer: Answer,
    /// A choice's options by probability, highest first; empty otherwise.
    pub distribution: Vec<(String, f64)>,
}

impl Weighed {
    pub fn unsure() -> Weighed {
        Weighed {
            answer: Answer::Unsure,
            distribution: Vec::new(),
        }
    }
}

pub trait Decider: Send + Sync {
    /// One call for the batch, one shared state; one answer a question, in order.
    fn ask(&self, qs: &[Question], cancel: &Cancel) -> Result<Vec<Weighed>>;
}
