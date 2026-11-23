//! One review pass: the state it carries and how it saves itself.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::{Error, Result};

use super::ask::ReviewResults;
use super::request_and_report::{Progress, ReviewDeps, ReviewReport};
use super::spent_so_far::SpentSoFar;
use crate::domain::budget::{Budget, Caps};
use crate::domain::config::ScryConfig;
use crate::domain::finding::{Finding, FindingId, Status};
use crate::domain::findings::{Findings, merge_into, renamed};
use crate::domain::scope::Scope;
use crate::domain::steering::Steering;

/// Why a pass stops before its end, writing nothing more.
pub(super) enum Halt {
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

/// One review pass: what it works with, and what it has done so far.
///
/// The pass changes a copy of the store (`working_copy`) as it goes and
/// writes it over the store twice: before each round's paid checks, and at
/// the end. Each write lays its changes over the store as it is then, so a
/// finding the user dismissed meanwhile stays dismissed.
pub(super) struct Pass<'r, 'd> {
    pub(super) cfg: &'r ScryConfig,
    pub(super) steering: &'r Steering,
    pub(super) fences: &'r Fences,
    pub(super) deps: &'r ReviewDeps<'d>,
    /// When the pass started: every status change and new finding is dated
    /// with it.
    pub(super) now: DateTime<Utc>,
    /// Each file's text, read once per pass.
    pub(super) file_texts: HashMap<PathBuf, String>,
    /// The store as it was when the pass started.
    pub(super) stored_before: Findings,
    /// The store as this pass will leave it.
    pub(super) working_copy: Findings,
    /// Stored findings a finding of this pass was merged into.
    pub(super) merged: HashSet<FindingId>,
    /// The findings this pass added, in the order it added them.
    pub(super) new: Vec<FindingId>,
    pub(super) budget: Budget,
    pub(super) review_estimate: f64,
    pub(super) check_estimate: f64,
    /// The store as this pass last wrote it (at first, as it read it), in
    /// the ids it was written with.
    pub(super) last_written: Findings,
    /// When another writer added a finding with the same id as one of this
    /// pass's new findings, the pass's finding is written under a fresh id:
    /// this maps the pass's id to the one written.
    pub(super) renamed_ids: HashMap<FindingId, FindingId>,
}

impl<'r, 'd> Pass<'r, 'd> {
    /// A pass that starts from the stored findings, within the spending
    /// limits, having spent what the ledger says so far today.
    pub(super) fn new(
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
    pub(super) fn run(
        mut self,
        scope: &Scope,
        fresh: bool,
    ) -> std::result::Result<ReviewReport, Halt> {
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
    pub(super) fn report(
        &self,
        review: ReviewResults,
        budget_ran_out: bool,
        scope: &Scope,
    ) -> ReviewReport {
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
    pub(super) fn day_held_something_back(&self, calls_capped: usize, checks_capped: bool) -> bool {
        (calls_capped > 0 && self.budget.day_is_spent(self.review_estimate))
            || (checks_capped && self.budget.day_is_spent(self.check_estimate))
    }

    /// Writes the working copy over the store as it is now: each finding
    /// takes the pass's version only if nobody else changed it since the
    /// pass last wrote, and the pass's new findings are added.
    pub(super) fn save_working_copy(&mut self) -> Result<()> {
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

    pub(super) fn log(&self, text: &str) {
        self.deps.log.put(text);
    }

    /// Tells the caller's progress callback what just happened.
    pub(super) fn tell_progress(&self, p: Progress) {
        (self.deps.progress)(&p);
    }

    /// The file's text, read once per pass; empty when the file is fenced
    /// off or cannot be read.
    pub(super) fn text(&mut self, file: &Path) -> String {
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
pub(super) fn ids_of(findings: &Findings) -> HashSet<FindingId> {
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
