//! A finding: one problem scry says a change has, with where it is, what
//! it cites as evidence, how sure scry is of it, and what became of it.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::unit::Span;

/// A finding's id: six lowercase hex digits, such as `3fa9c0`. It never
/// changes once the finding is stored.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FindingId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Shown: passed the checks and the confidence threshold.
    Open,
    /// Raised by the reviewer, not yet verified: the pass ran out of budget
    /// before checking it. Older stores call this "waiting".
    #[serde(alias = "waiting")]
    Unverified,
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

/// Where a finding is.
/// How far the user's dismissal of a finding reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dismissal {
    /// Only in the unit the finding is in: the same problem raised in
    /// another unit is shown again.
    Here,
    /// In every unit: the same problem is never shown again.
    Everywhere,
}

impl Dismissal {
    /// The status a finding dismissed this way takes.
    pub fn status(self) -> Status {
        match self {
            Dismissal::Here => Status::DismissedHere,
            Dismissal::Everywhere => Status::DismissedEverywhere,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub file: PathBuf,
    /// The name of the unit it is in, such as `Order.cancel`; none when its
    /// lines are in no unit.
    pub unit: Option<String>,
    pub lines: Span,
    /// The unit's first line when the finding was placed.
    pub unit_start: Option<u32>,
    /// The unit's fingerprint when the finding was placed, so scry can tell
    /// later whether the unit's code changed.
    pub fingerprint: Option<Fingerprint>,
}

/// Lines a finding cites to back its claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub file: PathBuf,
    pub lines: Span,
    /// The fingerprint of the cited code when the citation was verified.
    pub print: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    /// How sure Jev is that the finding is worth saying, from 0 to 1. It is
    /// shown when it reaches the project's `confidence_threshold`.
    pub confidence: Option<f64>,
    /// A stamp of what the nitpick question was asked about (the unit's
    /// code, the rule text, the question's wording), so it is asked again
    /// only when one of them changes.
    pub asked_on: Option<String>,
}

/// What the claim check said about a finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredVerdict {
    /// `right`, `wrong`, `cannot_tell`, or why its citations failed.
    pub verdict: String,
    pub why: String,
    /// What the check cited (or, for a citation failure, the finding's own
    /// evidence), each confirmed or not. Absent in stores written before it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cited: Vec<StoredCitation>,
}

/// One thing a verdict cites (a file, and maybe a unit and a line in it),
/// and whether it was found there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCitation {
    pub path: String,
    pub unit: Option<String>,
    pub line: Option<u32>,
    pub confirmed: bool,
    /// Confirmed on its line although its unit was not found. Absent when
    /// false, and in stores written before it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unit_missed: bool,
}

impl StoredCitation {
    /// Whether the citation was found, in words: `confirmed`, `not found`,
    /// or, when its unit was missing but its line held, `unit not found,
    /// confirmed on line 41`.
    pub fn outcome(&self) -> String {
        match (self.confirmed, self.unit_missed, self.line) {
            (true, true, Some(line)) => format!("unit not found, confirmed on line {line}"),
            (true, _, _) => "confirmed".into(),
            (false, _, _) => "not found".into(),
        }
    }

    /// The citation in words, with the parts it has: `orders.py`,
    /// `orders.py Order.cancel`, or `orders.py Order.cancel line 41`.
    pub fn label(&self) -> String {
        let unit = self.unit.as_ref().map(|unit| format!(" {unit}"));
        let line = self.line.map(|line| format!(" line {line}"));
        format!(
            "{}{}{}",
            self.path,
            unit.unwrap_or_default(),
            line.unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub id: FindingId,
    /// `bug`, `assumption`, `convention` or `design`.
    pub issue_type: String,
    /// The id of the project rule it breaks, if it cites one.
    pub rule: Option<String>,
    /// What is wrong and why it matters, in the reviewer's words.
    pub statement: String,
    pub place: Place,
    pub evidence: Vec<Evidence>,
    pub scores: Scores,
    pub verdict: Option<StoredVerdict>,
    pub status: Status,
    /// How many passes have raised it.
    pub seen: u32,
    /// When it was first raised.
    pub first: DateTime<Utc>,
    /// When it was last raised.
    pub last: DateTime<Utc>,
    /// Each status it has had, with when.
    pub history: Vec<(DateTime<Utc>, Status)>,
    /// The model that reviewed the code.
    pub model: String,
    /// Always 0 today: what a pass costs is in the run log.
    pub cost_usd: f64,
}

impl Finding {
    /// Changes the status and adds the change to the history.
    pub fn set_status(&mut self, status: Status, at: DateTime<Utc>) {
        self.status = status;
        self.history.push((at, status));
    }

    /// The finding, counted as seen once more, last at `at`.
    pub fn seen_again(self, at: DateTime<Utc>) -> Finding {
        Finding {
            seen: self.seen + 1,
            last: at,
            ..self
        }
    }
}

/// A status as the store writes it: `Status::DismissedHere` is
/// `dismissed-here`.
pub fn status_word(status: Status) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}
