//! A claim about code, the citations a check gives, and the verdict.

use std::path::PathBuf;

use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::unit::Span;

pub use crate::domain::claim_prompt::RawCitation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub file: PathBuf,
    pub lines: Span,
    /// The fingerprint of the cited lines, once confirmed.
    pub print: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimKind {
    Code,
    /// Checked against public web sources: a later milestone.
    Web,
}

#[derive(Debug, Clone)]
pub struct Claim {
    pub statement: String,
    /// Where the claim points, if anywhere (a hint for the checker).
    pub about: Vec<Citation>,
    pub kind: ClaimKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Right,
    Wrong,
    CannotTell,
}

/// A citation as the verdict's reply gave it, and whether it was confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    pub raw: RawCitation,
    pub confirmed: bool,
    /// Confirmed on its line although the unit it named was not found.
    pub unit_missed: bool,
}

#[derive(Debug, Clone)]
pub struct Checked {
    pub verdict: Verdict,
    pub why: String,
    /// The confirmed citations; empty when any did not confirm.
    pub citations: Vec<Citation>,
    /// Every citation of the reply the verdict came from, confirmed or not,
    /// so a `cannot_tell` can say which one failed.
    pub cited: Vec<Cited>,
    pub escalated: bool,
    pub errands: Vec<Errand>,
}

/// What the prose corpus measured: haiku first, sonnet on doubt, a 0.60 threshold.
pub const FIRST: &str = "haiku";
pub const STRONG: &str = "sonnet";
pub const THRESHOLD: f64 = 0.60;
pub const CITATION_MISSED: &str = "cited code not found";
