//! Reviewing a change: the `review_change` use case, what `scry` runs.
//!
//! `review_change` sets the review up (what to review, the spending
//! limits, the project's rules), locks the finding store, and runs one
//! `Pass` over it. The pass's steps live in their own files:
//!
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
//!
//! The run log's text is built last in the file.

mod ask;
mod judge;
mod remember;
pub mod scope;
pub mod verify;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sorcerer_core::domain::cost::spent_since;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::versioned::Snapshot;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Cancel, Error, Result};
use sorcerer_models::ports::decider::Decider;

use crate::domain::budget::{Budget, Caps, check_estimate, review_estimate};
use crate::domain::config::ScryConfig;
use crate::domain::finding::{Finding, FindingId, Status};
use crate::domain::findings::{Findings, merge_into, renamed};
use crate::domain::log_text::{count, scope_text, section, usd};
use crate::domain::scope::{Scope, ScopeRequest};
use crate::domain::steering::Steering;
use crate::ports::claim_checker::ClaimChecker;
use crate::ports::clock::Clock;
use crate::ports::finding_store::FindingStore;
use crate::ports::reviewer::Reviewer;
use crate::ports::run_log::RunLog;
use crate::ports::settings::Settings;
use ask::ReviewResults;
use scope::scope_of;

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

/// Why a pass stops before its end, writing nothing more.
enum Halt {
    /// Jev could not answer a question (an error, or "unsure"): scry does
    /// not run on a guess, so the review is skipped.
    JevDidNotAnswer,
    /// Something failed (the store, the disk): the review is an error.
    Failed(Error),
}

impl From<Error> for Halt {
    fn from(e: Error) -> Halt {
        Halt::Failed(e)
    }
}

/// Reviews the change the request names and verifies what the reviewer
/// found:
///
/// 1. reads `[scry]`; when scry is turned off, it skips;
/// 2. works out what to review (the scope) and logs it;
/// 3. works out the spending limits, and reads the project's rules
///    (steering) and the files scry must not read (fences);
/// 4. locks the finding store, so two reviews never run at once;
/// 5. reads the stored findings and what scry spent today, and logs them;
/// 6. runs the pass (review, judge, remember, verify) and saves its result
///    over the store as it is then;
/// 7. tells the caller the store was written, and unlocks it.
///
/// When Jev cannot answer one of its questions the whole review is skipped
/// and nothing is written: scry does not run on a guess.
pub fn review_change(
    project: &Project,
    req: &ReviewRequest,
    deps: &ReviewDeps,
) -> Result<ReviewOutcome> {
    let cfg = deps.settings.scry_config()?;
    if !cfg.enabled {
        return Ok(ReviewOutcome::Skipped(SCRY_IS_OFF.into()));
    }
    let scope = scope_of(project, deps.view, req.scope.clone(), req.all, &cfg)?;
    deps.log.put(&scope_text(&scope));
    let caps = caps_for(req, &cfg);
    let steering = deps.settings.steering()?;
    let fences = Fences::for_project(project)?;

    let store_lock = deps.store.hold()?;
    let stored_before = findings_on_disk(deps.store)?;
    let spent = SpentSoFar::read(deps.ledger, deps.clock, &cfg)?;
    deps.log
        .put(&pass_text(&caps, &spent, &cfg.review_model, &stored_before));
    let pass = Pass::new(&cfg, &steering, &fences, deps, &caps, &spent, stored_before);
    let report = match pass.run(&scope, req.fresh) {
        Ok(report) => report,
        Err(Halt::JevDidNotAnswer) => return Ok(ReviewOutcome::Skipped(JEV_DID_NOT_ANSWER.into())),
        Err(Halt::Failed(e)) => return Err(e),
    };
    (deps.progress)(&Progress::Stored);
    drop(store_lock);
    Ok(ReviewOutcome::Ran(report))
}

/// The spending limits: the request's own when it gives them (the editor,
/// tests); else the command line's: no limit per pass, `[scry]
/// budget_per_day` for the day, and `--budget` when given.
fn caps_for(req: &ReviewRequest, cfg: &ScryConfig) -> Caps {
    req.caps.clone().unwrap_or(Caps {
        pass: None,
        day: cfg.budget_per_day,
        budget: req.budget,
    })
}

/// The stored findings: none when there is no store yet. A store scry
/// cannot read is an error, so the pass never writes over it.
fn findings_on_disk(store: &dyn FindingStore) -> Result<Findings> {
    match store.read()? {
        Snapshot::Absent => Ok(Findings::default()),
        Snapshot::Current(s) => Ok(s),
        Snapshot::Unreadable(why) => Err(Error::StoreUnreadable {
            path: store.path().to_path_buf(),
            why,
        }),
    }
}

/// What scry spent earlier in the local day, and what a review call and a
/// claim check are estimated to cost, as of the moment the pass starts.
struct SpentSoFar {
    as_of: DateTime<Utc>,
    today: f64,
    review_estimate: f64,
    check_estimate: f64,
}

impl SpentSoFar {
    /// Reads the ledger (every paid call scry made) as of the clock's now.
    fn read(ledger: &dyn Ledger, clock: &dyn Clock, cfg: &ScryConfig) -> Result<SpentSoFar> {
        let rows = ledger.read()?.errands;
        let now = clock.now();
        let offset = clock.offset();
        // Jev's questions are not counted against the caps: a fraction of a cent each
        let paid: Vec<_> = rows
            .iter()
            .filter(|e| e.verb == "review" || e.verb == "check")
            .cloned()
            .collect();
        Ok(SpentSoFar {
            as_of: now,
            today: spent_since(
                &paid,
                "scry",
                now.with_timezone(&offset).date_naive(),
                offset,
            ),
            review_estimate: review_estimate(&rows, &cfg.review_model),
            check_estimate: check_estimate(&rows),
        })
    }
}

/// A unit sent to the review call: its file, its name and the fingerprint
/// it was sent at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentUnit {
    pub file: PathBuf,
    pub unit: String,
    pub fingerprint: Fingerprint,
}

/// One review pass: what it works with, and what it has done so far.
///
/// The pass changes a copy of the store (`working_copy`) as it goes and
/// writes it over the store twice: before each round's paid checks, and at
/// the end. Each write lays its changes over the store as it is then, so a
/// finding the user dismissed meanwhile stays dismissed.
struct Pass<'r, 'd> {
    cfg: &'r ScryConfig,
    steering: &'r Steering,
    fences: &'r Fences,
    deps: &'r ReviewDeps<'d>,
    /// When the pass started: every status change and new finding is dated
    /// with it.
    now: DateTime<Utc>,
    /// Each file's text, read once per pass.
    file_texts: HashMap<PathBuf, String>,
    /// The store as it was when the pass started.
    stored_before: Findings,
    /// The store as this pass will leave it.
    working_copy: Findings,
    /// Stored findings a finding of this pass was merged into.
    merged: HashSet<FindingId>,
    /// The findings this pass added, in the order it added them.
    new: Vec<FindingId>,
    budget: Budget,
    review_estimate: f64,
    check_estimate: f64,
    /// The store as this pass last wrote it (at first, as it read it), in
    /// the ids it was written with.
    last_written: Findings,
    /// When another writer added a finding with the same id as one of this
    /// pass's new findings, the pass's finding is written under a fresh id:
    /// this maps the pass's id to the one written.
    renamed_ids: HashMap<FindingId, FindingId>,
}

impl<'r, 'd> Pass<'r, 'd> {
    /// A pass that starts from the stored findings, within the spending
    /// limits, having spent what the ledger says so far today.
    fn new(
        cfg: &'r ScryConfig,
        steering: &'r Steering,
        fences: &'r Fences,
        deps: &'r ReviewDeps<'d>,
        caps: &Caps,
        spent: &SpentSoFar,
        stored_before: Findings,
    ) -> Pass<'r, 'd> {
        Pass {
            cfg,
            steering,
            fences,
            deps,
            now: spent.as_of,
            file_texts: HashMap::new(),
            working_copy: stored_before.clone(),
            last_written: stored_before.clone(),
            stored_before,
            merged: HashSet::new(),
            new: Vec::new(),
            budget: Budget::new(caps, spent.today),
            review_estimate: spent.review_estimate,
            check_estimate: spent.check_estimate,
            renamed_ids: HashMap::new(),
        }
    }

    /// Runs the pass over the scope and saves it:
    ///
    /// 1. findings whose unit was deleted are gone, and findings on a unit
    ///    put back to the base's code are resolved;
    /// 2. the units not yet reviewed (all of them when `fresh`) go to the
    ///    reviewer, in batches;
    /// 3. each finding it raised is judged (same as a stored one? a
    ///    nitpick?);
    /// 4. what the review settled is remembered;
    /// 5. the unverified findings are verified, file by file;
    /// 6. the units the reviewer answered for are marked reviewed;
    /// 7. the working copy is saved over the store, and the report made.
    fn run(mut self, scope: &Scope, fresh: bool) -> std::result::Result<ReviewReport, Halt> {
        let unverified_from_earlier_runs = unverified_ids(&self.stored_before);
        let gone = self.sweep_gone(scope);
        let put_back = self.sweep_put_back(scope);
        let review = self.send(scope, fresh);
        self.judge_all(review.raw_findings.clone(), scope)?;
        self.remember(&review, &gone, &put_back);
        let budget_ran_out = self.verify_findings_per_file(scope, &unverified_from_earlier_runs)?;
        self.mark_reviewed(&review.answered);
        self.save_working_copy()?;
        Ok(self.report(review, budget_ran_out, scope))
    }

    /// What the pass did, for the caller. Ids are the ones written to the
    /// store (see `renamed_ids`).
    fn report(&self, review: ReviewResults, budget_ran_out: bool, scope: &Scope) -> ReviewReport {
        let changed = touched_since(&self.stored_before, &self.working_copy);
        ReviewReport {
            looked_at: review.answered.len(),
            new: as_written(&self.new, &self.renamed_ids),
            spent: self.budget.spent(),
            capped: review.capped,
            whole: review.every_unit_was_reviewed(),
            failures: review.failures,
            checks_capped: budget_ran_out,
            day_spent: self.day_held_something_back(review.capped, budget_ran_out),
            touched: as_written(&changed, &self.renamed_ids),
            scope: scope.clone(),
        }
    }

    /// Whether the day's cap is what held back a review call or a claim
    /// check: the next one would take the day over its cap.
    fn day_held_something_back(&self, calls_capped: usize, checks_capped: bool) -> bool {
        (calls_capped > 0 && self.budget.day_is_spent(self.review_estimate))
            || (checks_capped && self.budget.day_is_spent(self.check_estimate))
    }

    /// Writes the working copy over the store as it is now: each finding
    /// takes the pass's version only if nobody else changed it since the
    /// pass last wrote, and the pass's new findings are added.
    fn save_working_copy(&mut self) -> Result<()> {
        self.deps.store.update(&mut |current| {
            *current = merge_into(
                &self.last_written,
                &self.working_copy,
                current,
                &mut self.renamed_ids,
            );
        })?;
        self.last_written = renamed(&self.working_copy, &self.renamed_ids);
        Ok(())
    }

    fn log(&self, text: &str) {
        self.deps.log.put(text);
    }

    /// Tells the caller's progress callback what just happened.
    fn tell_progress(&self, p: Progress) {
        (self.deps.progress)(&p);
    }

    /// The file's text, read once per pass; empty when the file is fenced
    /// off or cannot be read.
    fn text(&mut self, file: &Path) -> String {
        if let Some(t) = self.file_texts.get(file) {
            return t.clone();
        }
        let t = if self.fences.allows(file) {
            self.deps.view.text(file).unwrap_or_default()
        } else {
            String::new()
        };
        self.file_texts.insert(file.to_path_buf(), t.clone());
        t
    }
}

/// The ids of the findings.
fn ids_of(findings: &Findings) -> HashSet<FindingId> {
    findings.findings.iter().map(|f| f.id.clone()).collect()
}

/// The findings nobody has verified yet.
fn unverified_ids(findings: &Findings) -> HashSet<FindingId> {
    findings
        .findings
        .iter()
        .filter(|f| f.status == Status::Unverified)
        .map(|f| f.id.clone())
        .collect()
}

/// The findings in `now` that are new or not as they were in `before`, in
/// store order.
fn touched_since(before: &Findings, now: &Findings) -> Vec<FindingId> {
    let was: HashMap<&FindingId, &Finding> = before.findings.iter().map(|f| (&f.id, f)).collect();
    now.findings
        .iter()
        .filter(|f| was.get(&f.id) != Some(f))
        .map(|f| f.id.clone())
        .collect()
}

/// The ids as they were written: a renamed one under its new id.
fn as_written(ids: &[FindingId], renamed_ids: &HashMap<FindingId, FindingId>) -> Vec<FindingId> {
    ids.iter()
        .map(|id| renamed_ids.get(id).unwrap_or(id).clone())
        .collect()
}

// The run log's text, and the reasons a review is skipped.

const SCRY_IS_OFF: &str = "scry is off ([scry] enabled = false)";

const JEV_DID_NOT_ANSWER: &str = "Jev did not answer";

/// The `Pass` section: the caps, what scry spent earlier today, the
/// estimates and the store as the pass found it.
fn pass_text(caps: &Caps, spent: &SpentSoFar, review_model: &str, start: &Findings) -> String {
    format!(
        "{}- caps: pass {}, day {} (scry spent {} earlier today), budget {}\n\
         - estimates: review {} ({}), check {}\n\
         - store: {}, {} reviewed before\n",
        section("Pass"),
        caps.pass.map_or("none".into(), usd),
        usd(caps.day),
        usd(spent.today),
        caps.budget.map_or("none".into(), usd),
        usd(spent.review_estimate),
        review_model,
        usd(spent.check_estimate),
        count(start.findings.len(), "finding"),
        count(start.reviewed.len(), "file"),
    )
}
