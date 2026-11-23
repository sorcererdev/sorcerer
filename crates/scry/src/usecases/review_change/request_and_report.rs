//! What a review is asked, what it talks through, and what it tells back.

use std::path::PathBuf;

use sorcerer_core::Cancel;
use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::ports::decider::Decider;

use crate::domain::budget::Caps;
use crate::domain::finding::FindingId;
use crate::domain::scope::{Scope, ScopeRequest};
use crate::ports::claim_checker::ClaimChecker;
use crate::ports::clock::Clock;
use crate::ports::finding_store::FindingStore;
use crate::ports::reviewer::Reviewer;
use crate::ports::run_log::RunLog;
use crate::ports::settings::Settings;

/// What a review is told as it goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// A review call is about to go out with these units: told once per call, after the
    /// spending caps and the cancel let it go, right before it is sent.
    Sending(Vec<SentUnit>),
    /// A review call answered with this many findings.
    Reviewed(usize),
    /// The store was written.
    Stored,
}

/// What to review: the files (`--since`, named, or the working tree), every
/// unit or only the changed ones, whether to send units already reviewed
/// again, and the caps.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewRequest {
    pub scope: ScopeRequest,
    /// Every unit of each file in scope, not only the added and changed ones.
    pub all: bool,
    /// Send units already reviewed at their fingerprint again.
    pub fresh: bool,
    /// The most this run may spend (the command line's `--budget`).
    pub budget: Option<f64>,
    /// The caps exactly (an editor's pass cap; tests). None: the command
    /// line's, no pass cap, the day's `[scry] budget_per_day` and BUDGET.
    pub caps: Option<Caps>,
}

impl ReviewRequest {
    /// SCOPE's changed units, as the command line runs it.
    pub fn of(scope: ScopeRequest) -> ReviewRequest {
        ReviewRequest {
            scope,
            all: false,
            fresh: false,
            budget: None,
            caps: None,
        }
    }
}

/// What a review talks through.
pub struct ReviewDeps<'a> {
    pub view: &'a dyn CodebaseView,
    pub reviewer: &'a dyn Reviewer,
    pub decider: &'a dyn Decider,
    pub checker: &'a dyn ClaimChecker,
    pub store: &'a dyn FindingStore,
    pub ledger: &'a dyn Ledger,
    pub settings: &'a dyn Settings,
    pub clock: &'a dyn Clock,
    /// The run log: each step's inputs and decisions, as Markdown.
    pub log: &'a dyn RunLog,
    pub progress: &'a (dyn Fn(&Progress) + Sync),
    /// Stops Jev's questions and the claim checks: set when the run is stopped.
    pub cancel: &'a Cancel,
    /// Stops the review calls only, the one in flight and those not yet sent. An editor sets
    /// it when a save overtakes the code a call was sent; the command line passes `cancel`.
    pub review_cancel: &'a Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewReport {
    /// Units sent to a review call that answered.
    pub looked_at: usize,
    /// Findings this pass added to the store.
    pub new: Vec<FindingId>,
    pub spent: f64,
    /// Units a cap held back.
    pub capped: usize,
    /// One line per review call that failed.
    pub failures: Vec<String>,
    /// Nothing capped and every review call answered.
    pub whole: bool,
    /// A claim check did not fit the caps; its finding stays unverified.
    pub checks_capped: bool,
    /// Something was held back (a review call or a claim check) and the day's cap is what
    /// held it: the next call would take the day over `budget_per_day`.
    pub day_spent: bool,
    /// Every finding this pass added or changed, in store order.
    pub touched: Vec<FindingId>,
    /// What was in scope.
    pub scope: Scope,
}

// One a run: the report is not worth a box.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum ReviewOutcome {
    Skipped(String),
    Ran(ReviewReport),
}

/// A unit sent to the review call: its file, its name and the fingerprint
/// it was sent at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentUnit {
    pub file: PathBuf,
    pub unit: String,
    pub fingerprint: Fingerprint,
}
