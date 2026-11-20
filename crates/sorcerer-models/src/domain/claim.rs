//! A claim about code, the places a check cites for its verdict, and the
//! verdict itself.

use std::path::PathBuf;

use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::unit::Span;

pub use crate::domain::claim_prompt::RawCitation;

/// A place in the project: a file and a range of its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub file: PathBuf,
    pub lines: Span,
    /// The fingerprint of the cited lines, once the place was found, so a
    /// later edit to them can be noticed.
    pub print: Option<String>,
}

/// What a claim is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimKind {
    /// The project's own code.
    Code,
    /// Public web sources: not checked yet, a later milestone.
    Web,
}

/// A statement to check, such as "`cancel` returns `None` when the order
/// is missing".
#[derive(Debug, Clone)]
pub struct Claim {
    pub statement: String,
    /// Places the claim is about, if it names any; the checker starts there.
    pub about: Vec<Citation>,
    pub kind: ClaimKind,
}

/// Whether the claim holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Right,
    Wrong,
    /// The checker could not decide, or a place it cited was not found.
    CannotTell,
}

/// Whether a place the checker cited was found in the project (confirmed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitationCheck {
    /// Found: the function (or other unit) it names, or, when it names
    /// none, the line it gives.
    Confirmed,
    /// The unit it names is not in the file, but the line it gives is, so
    /// the citation stands on that one line.
    ConfirmedOnTheLineOnly,
    /// Not found, or outside what the checker may read.
    NotConfirmed,
}

/// A citation as the checker's reply gave it, and whether it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    pub raw: RawCitation,
    pub check: CitationCheck,
}

/// Whether the check went on to the strong model. It does when the cheap
/// model's verdict was not trusted (see `check_claim`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escalation {
    No,
    ToTheStrongModel,
}

/// The outcome of checking one claim.
#[derive(Debug, Clone)]
pub struct Checked {
    pub verdict: Verdict,
    /// The checker's reason, in its own words.
    pub why: String,
    /// The places the verdict rests on, all found; empty when any was not.
    pub citations: Vec<Citation>,
    /// Every place the verdict's reply cited, found or not, so a
    /// `cannot_tell` can say which one was missing.
    pub cited: Vec<Cited>,
    pub escalation: Escalation,
    /// The ledger rows of the model calls the check made.
    pub errands: Vec<Errand>,
}

/// The cheap model that checks first.
pub const FIRST_MODEL: &str = "haiku";
/// The model that checks when the first one's verdict is not trusted.
pub const STRONG_MODEL: &str = "sonnet";
/// How sure each of Jev's guards must be (0.60) for the first model's
/// verdict to stand. Haiku first, sonnet on doubt and 0.60 are what the
/// prose corpus measured best.
pub const GUARD_THRESHOLD: f64 = 0.60;
/// The reason given when a cited place was not found.
pub const CITATION_MISSED: &str = "cited code not found";
