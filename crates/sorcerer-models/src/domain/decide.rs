//! The decider: Jev, or anything that answers typed questions (yes or no,
//! a choice, a level) with calibrated probabilities. It never writes text.
//! A calibrated yes/no costs a fraction of a cent, answers in under a
//! second, and gives a probability a cut can be put on.

/// The kind of answer a question wants.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// Pick one of these options, each a name and what it means.
    /// Example: `[("bug", "the code does the wrong thing"), ("style", "...")]`.
    Choice(Vec<(String, String)>),
    /// Yes or no (Jev calls this a "noul"); `yes` and `no` say what each
    /// side means, such as yes = "only a matter of taste".
    Noul { yes: String, no: String },
    /// A level on a scale, one description per level, starting at 0.
    Score(Vec<String>),
}

/// Whether a choice may be answered "none of these fits" instead of one of
/// its options. Yes/no and score questions ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsureAnswer {
    Allowed,
    NotAllowed,
}

/// One question for the decider.
///
/// Example: a yes/no question named `nitpick`, with the instructions "Is
/// this finding only a matter of taste?" and the context `finding` = the
/// finding's text and `code` = the lines it is about.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// The question's name in its batch (`nitpick`, `same-problem-3`).
    pub kind: String,
    pub instructions: String,
    pub shape: Shape,
    /// The texts the question is about, each under a name. Questions asked
    /// together share their context, so a name used by two of them must
    /// hold the same text in both.
    pub context: Vec<(String, String)>,
    pub unsure: UnsureAnswer,
}

/// How sure the decider is of its answer: a probability from 0 to 1, or
/// unknown when it gave none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Confidence {
    Known(f64),
    Unknown,
}

/// The decider's answer to one question.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// The option it picked, by name.
    Chosen(String, Confidence),
    Yes(Confidence),
    No(Confidence),
    /// The level it picked, its score on the scale (2.6 lies between
    /// levels 2 and 3), and how sure it is.
    Level(u8, f64, Confidence),
    /// "None of these fits", or no answer at all.
    Unsure,
}

impl Answer {
    /// The probability that the answer is yes, for a yes/no answer with a
    /// known confidence: a "no" at 0.8 is a yes at 0.2. Any other answer
    /// has none.
    pub fn p_yes(&self) -> Option<f64> {
        match self {
            Answer::Yes(Confidence::Known(p)) => Some(*p),
            Answer::No(Confidence::Known(p)) => Some(1.0 - p),
            _ => None,
        }
    }
}

/// An answer with, for a choice, how likely each option was.
#[derive(Debug, Clone, PartialEq)]
pub struct Weighed {
    pub answer: Answer,
    /// A choice's options with their probabilities, most likely first,
    /// such as `[("bug", 0.7), ("style", 0.3)]`; empty for other shapes.
    pub distribution: Vec<(String, f64)>,
}

impl Weighed {
    /// The answer when the decider could not answer.
    pub fn unsure() -> Weighed {
        Weighed {
            answer: Answer::Unsure,
            distribution: Vec::new(),
        }
    }
}
