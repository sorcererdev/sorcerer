//! The review_change use case:
//! the request's scope worked out; the scope's units not yet reviewed go to
//! the review call in batches (`ask`); each finding is placed, matched against
//! the store, filtered by Jev's nitpick question and remembered (`judge`,
//! `remember`); then each file's waiting findings go through the
//! verification rounds (`verify`), within the caps.

mod ask;
mod judge;
mod remember;
pub mod scope;
pub mod verify;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sorcerer_core::domain::cost::spent_in;
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
use crate::domain::findings::{Findings, merge_into};
use crate::domain::log_text::{count, scope_text, section, usd};
use crate::domain::scope::{Scope, ScopeRequest};
use crate::domain::steering::Steering;
use crate::ports::claim_checker::ClaimChecker;
use crate::ports::clock::Clock;
use crate::ports::finding_store::FindingStore;
use crate::ports::reviewer::Reviewer;
use crate::ports::run_log::RunLog;
use crate::ports::settings::Settings;
use scope::scope_of;

/// What a review is told as it goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
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
    pub cancel: &'a Cancel,
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
    /// A claim check did not fit the caps; its finding waits.
    pub checks_capped: bool,
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

/// Why a pass stops early: Jev gave no answer (scry does not run on a
/// guess), or an error.
enum Halt {
    Silent,
    Failed(Error),
}

impl From<Error> for Halt {
    fn from(e: Error) -> Halt {
        Halt::Failed(e)
    }
}

/// One pass over the scope REQ names. Holds the store's pass lock
/// throughout; writes the store before a round's claim checks start and at
/// the end, each time laying its changes over the store as it is then. The
/// caps hold the review calls and claim checks back, the day's cap counting
/// what scry spent earlier in the local day.
#[expect(clippy::too_many_lines, reason = "readability stage 2")]
pub fn review_change(
    project: &Project,
    req: &ReviewRequest,
    deps: &ReviewDeps,
) -> Result<ReviewOutcome> {
    let cfg = deps.settings.scry_config()?;
    if !cfg.enabled {
        return Ok(ReviewOutcome::Skipped(
            "scry is off ([scry] enabled = false)".into(),
        ));
    }
    let scope = scope_of(project, deps.view, req.scope.clone(), req.all, &cfg)?;
    deps.log.put(&scope_text(&scope));
    let caps = req.caps.clone().unwrap_or(Caps {
        pass: None,
        day: cfg.budget_per_day,
        budget: req.budget,
    });
    let caps = &caps;
    let st = deps.settings.steering()?;
    let fences = Fences::for_project(project)?;
    let held = deps.store.hold()?;
    let base = match deps.store.read()? {
        Snapshot::Absent => Findings::default(),
        Snapshot::Current(s) => s,
        Snapshot::Unreadable(why) => {
            return Err(Error::StoreUnreadable {
                path: deps.store.path().to_path_buf(),
                why,
            });
        }
    };
    let rows = deps.ledger.read()?.errands;
    let now = deps.clock.now();
    let offset = deps.clock.offset();
    // Jev's questions are not counted against the caps: a fraction of a cent each
    let paid: Vec<_> = rows
        .iter()
        .filter(|e| e.verb == "review" || e.verb == "check")
        .cloned()
        .collect();
    let today = spent_in(
        &paid,
        "scry",
        now.with_timezone(&offset).date_naive(),
        offset,
    );
    let (review_est, check_est) = (
        review_estimate(&rows, &cfg.review_model),
        check_estimate(&rows),
    );
    deps.log.put(&format!(
        "{}- caps: pass {}, day {} (scry spent {} earlier today), budget {}\n\
         - estimates: review {} ({}), check {}\n\
         - store: {}, {} reviewed before\n",
        section("Pass"),
        caps.pass.map_or("none".into(), usd),
        usd(caps.day),
        usd(today),
        caps.budget.map_or("none".into(), usd),
        usd(review_est),
        cfg.review_model,
        usd(check_est),
        count(base.findings.len(), "finding"),
        count(base.reviewed.len(), "file"),
    ));
    let start = base.clone();
    let mut run = Run {
        cfg: &cfg,
        st: &st,
        fences: &fences,
        deps,
        cancel: deps.cancel,
        now,
        texts: HashMap::new(),
        mine: base.clone(),
        merged: HashSet::new(),
        new: Vec::new(),
        budget: Budget::new(caps, today),
        review_estimate: review_est,
        check_estimate: check_est,
        disk: deps.store,
        written: base,
        renames: HashMap::new(),
    };
    let report = match run.go(&scope, req.fresh) {
        Ok(r) => r,
        Err(Halt::Silent) => return Ok(ReviewOutcome::Skipped("Jev did not answer".into())),
        Err(Halt::Failed(e)) => return Err(e),
    };
    let (written, mine, mut renames) = (run.written, run.mine, run.renames);
    deps.store
        .update(&mut |current| *current = merge_into(&written, &mine, current, &mut renames))?;
    let mut report = report;
    let was: HashMap<&FindingId, &Finding> = start.findings.iter().map(|f| (&f.id, f)).collect();
    report.touched = mine
        .findings
        .iter()
        .filter(|f| was.get(&f.id) != Some(f))
        .map(|f| f.id.clone())
        .collect();
    for id in report.new.iter_mut().chain(report.touched.iter_mut()) {
        if let Some(to) = renames.get(id) {
            *id = to.clone();
        }
    }
    (deps.progress)(&Progress::Stored);
    drop(held);
    Ok(ReviewOutcome::Ran(report))
}

/// A unit sent to the review call: its file, name, first line and the
/// fingerprint it was sent at.
type Sent = (PathBuf, String, u32, Fingerprint);

struct Run<'r, 'd> {
    cfg: &'r ScryConfig,
    st: &'r Steering,
    fences: &'r Fences,
    deps: &'r ReviewDeps<'d>,
    cancel: &'r Cancel,
    now: DateTime<Utc>,
    texts: HashMap<PathBuf, String>,
    /// The store as this pass leaves it.
    mine: Findings,
    /// Stored findings a finding of this pass merged into.
    merged: HashSet<FindingId>,
    new: Vec<FindingId>,
    budget: Budget,
    review_estimate: f64,
    check_estimate: f64,
    disk: &'r dyn FindingStore,
    /// The store as this pass last wrote it (at first, as it read it), in
    /// written ids.
    written: Findings,
    /// The pass's ids that clashed with findings added meanwhile, to the
    /// ids written in their place.
    renames: HashMap<FindingId, FindingId>,
}

impl Run<'_, '_> {
    fn go(&mut self, scope: &Scope, fresh: bool) -> std::result::Result<ReviewReport, Halt> {
        let base_ids: HashSet<FindingId> =
            self.mine.findings.iter().map(|f| f.id.clone()).collect();
        let old_waiting: HashSet<FindingId> = self
            .mine
            .findings
            .iter()
            .filter(|f| f.status == Status::Waiting)
            .map(|f| f.id.clone())
            .collect();
        let gone = self.sweep_gone(scope);
        let mut asked = self.send(scope, fresh);
        self.judge_all(std::mem::take(&mut asked.raws), scope)?;
        let whole = asked.capped == 0 && asked.failures.is_empty();
        self.remember(&asked, &gone, &base_ids, whole);
        let checks_capped = self.verify_files(scope, &old_waiting)?;
        self.mark_reviewed(&asked.answered);
        Ok(ReviewReport {
            looked_at: asked.answered.len(),
            new: self.new.clone(),
            spent: self.budget.spent(),
            capped: asked.capped,
            failures: asked.failures,
            whole,
            checks_capped,
            touched: Vec::new(),
            scope: scope.clone(),
        })
    }

    fn t(&self, text: &str) {
        self.deps.log.put(text);
    }

    /// Tells the caller's progress callback that P happened.
    fn report(&self, p: Progress) {
        (self.deps.progress)(&p);
    }

    /// The file's text, read once a pass; empty when fenced or unreadable.
    fn text(&mut self, file: &Path) -> String {
        if let Some(t) = self.texts.get(file) {
            return t.clone();
        }
        let t = if self.fences.allows(file) {
            self.deps.view.text(file).unwrap_or_default()
        } else {
            String::new()
        };
        self.texts.insert(file.to_path_buf(), t.clone());
        t
    }
}
