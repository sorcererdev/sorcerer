//! Reviewing a change: the `review_change` use case, what `scry` runs.
//!
//! `review_change` sets the review up (what to review, the spending
//! limits, the project's rules), locks the finding store, and runs one
//! `Pass` over it. The pass's steps live in their own files:
//!
//! - `run_review.rs`: `review_change` itself: set up, lock, run, report;
//! - `request_and_report.rs`: the request, the dependencies and the report;
//! - `spent_so_far.rs`: what scry spent earlier today;
//! - `review_pass.rs`: the `Pass` state and its saving;
//! - `scope.rs`: which files and units to review;
//! - `ask.rs`: sending the units to the reviewer, in batches;
//! - `judge.rs`: what to do with each finding the reviewer raised;
//! - `remember.rs`: what the review settled, and the units now reviewed;
//! - `verify.rs`: checking the unverified findings, file by file.
//!
//! Example: you changed `cancel` in `orders.py` and run `scry`. The pass
//! sends `cancel` to the reviewer, which raises one bug; Jev says it is new
//! and not a nitpick, so it is stored unverified; the rounds check its
//! cited lines and ask the model whether it is true; it is, so it opens and
//! `scry` prints it.

mod ask;
mod judge;
mod remember;
mod request_and_report;
mod review_pass;
mod run_review;
pub mod scope;
mod spent_so_far;
pub mod verify;

pub use request_and_report::{
    Progress, ReviewDeps, ReviewOutcome, ReviewReport, ReviewRequest, SentUnit,
};
use review_pass::{Halt, Pass, ids_of};
pub use run_review::review_change;
