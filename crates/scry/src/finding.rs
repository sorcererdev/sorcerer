//! The finding: one thing scry says a change gets wrong, with where it is,
//! what it cites, how it scored and what became of it.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::fingerprint::Fingerprint;
use sorcerer_core::unit::Span;

/// Six lowercase hex digits, stable for the record's life.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Shown: passed the checks and the nitpick bar.
    Open,
    /// Found, not yet checked (a cap held the pass back).
    Waiting,
    /// Judged a nitpick: kept, not shown.
    Withheld,
    /// The claim checker said the claim is wrong.
    Refuted,
    /// The checker could not tell, or the citations did not hold.
    Unsure,
    /// Its lines are in no unit in scope.
    Elsewhere,
    /// The user dismissed it for this unit.
    DismissedHere,
    /// The user dismissed it everywhere.
    DismissedEverywhere,
    /// The code changed and the finding no longer holds.
    Resolved,
    /// Its unit no longer exists.
    Gone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub file: PathBuf,
    pub unit: Option<String>,
    pub lines: Span,
    pub unit_start: Option<u32>,
    pub fingerprint: Option<Fingerprint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub file: PathBuf,
    pub lines: Span,
    /// The fingerprint of the cited code when the citation was verified.
    pub print: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    pub keep: Option<f64>,
    /// What the nitpick judgement was asked about (unit, rule text, wording).
    pub asked_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredVerdict {
    /// `right`, `wrong`, `cannot_tell`, or the citation failure.
    pub verdict: String,
    pub why: String,
    /// What the check cited (or, for a citation failure, the finding's own
    /// evidence), each confirmed or not. Absent in stores written before it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cited: Vec<StoredCitation>,
}

/// One citation behind a verdict: a path, a unit or a line, and whether it
/// was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCitation {
    pub path: String,
    pub unit: Option<String>,
    pub line: Option<u32>,
    pub confirmed: bool,
}

impl StoredCitation {
    /// `path unit line N`, the parts it has.
    pub fn label(&self) -> String {
        let mut s = self.path.clone();
        if let Some(u) = &self.unit {
            s.push(' ');
            s.push_str(u);
        }
        if let Some(l) = self.line {
            s.push_str(&format!(" line {l}"));
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub id: Id,
    pub kind: String,
    pub rule: Option<String>,
    pub statement: String,
    pub place: Place,
    pub evidence: Vec<Evidence>,
    pub scores: Scores,
    pub verdict: Option<StoredVerdict>,
    pub status: Status,
    pub seen: u32,
    pub first: DateTime<Utc>,
    pub last: DateTime<Utc>,
    pub history: Vec<(DateTime<Utc>, Status)>,
    pub model: String,
    pub cost_usd: f64,
}

impl Finding {
    /// Change the status and record when.
    pub fn set_status(&mut self, s: Status, at: DateTime<Utc>) {
        self.status = s;
        self.history.push((at, s));
    }
}
