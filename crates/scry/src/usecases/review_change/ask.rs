//! Sending the units to review to the reviewer, and collecting what it
//! said.
//!
//! `send` picks the units not reviewed yet, groups them into batches of at
//! most 600 lines (one call each), and takes the batches in order. Each
//! batch is held back for a later run when the spending limit leaves no
//! room, failed when the review calls were cancelled, or else sent: `review_one_batch`
//! asks the reviewer and records its findings and its answers on the open
//! findings it was shown. What happened to each batch (`BatchOutcome`) is
//! added up into `ReviewResults`.
//!
//! The run log's text is built last in the file.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::Unit;

use super::{Pass, Progress, SentUnit};
use crate::domain::conjure_drops::{ConjureDrop, still_dropped};
use crate::domain::finding::{Finding, Status};
use crate::domain::log_text::{block, count, section, sub, usd};
use crate::domain::review_prompt::{
    Holds, RawFinding, ReviewAsk, ReviewReply, ReviewUnit, batch_by_lines, parse_review,
    review_prompt,
};
use crate::domain::scope::{Scope, ScopedUnit};
use crate::ports::reviewer::Reviewed;

/// What the review calls gave, added up over all the batches.
#[derive(Default)]
pub(super) struct ReviewResults {
    /// The units the reviewer answered for, at the version they were sent.
    pub answered: Vec<SentUnit>,
    /// The findings the reviewer raised.
    pub raw_findings: Vec<RawFinding>,
    /// The ids of open findings the reviewer said no longer hold.
    pub no_longer_hold: HashSet<String>,
    /// The ids of open findings the reviewer said still hold.
    pub still_hold: HashSet<String>,
    /// How many units a spending limit held back for a later run.
    pub capped: usize,
    /// One line for each call that failed or was cancelled.
    pub failures: Vec<String>,
}

impl ReviewResults {
    /// Whether every unit that needed a review got one: no unit was held
    /// back by a spending limit and no review call failed. Only then can a
    /// finding the reviewer did not raise again be resolved.
    pub(super) fn every_unit_was_reviewed(&self) -> bool {
        self.capped == 0 && self.failures.is_empty()
    }

    /// These results with one more batch's outcome added.
    fn with(self, outcome: BatchOutcome) -> ReviewResults {
        match outcome {
            BatchOutcome::HeldBack { units } => ReviewResults {
                capped: self.capped + units,
                ..self
            },
            BatchOutcome::Failed(line) => ReviewResults {
                failures: [self.failures, vec![line]].concat(),
                ..self
            },
            BatchOutcome::Answered {
                units,
                reply,
                shown,
            } => {
                // an answer counts only for a finding this call was shown
                let (still_hold, no_longer_hold): (Vec<_>, Vec<_>) = reply
                    .open
                    .into_iter()
                    .filter(|o| shown.contains(&o.id))
                    .partition(|o| o.holds == Holds::Still);
                ReviewResults {
                    answered: [self.answered, units].concat(),
                    raw_findings: [self.raw_findings, reply.findings].concat(),
                    still_hold: self
                        .still_hold
                        .into_iter()
                        .chain(still_hold.into_iter().map(|o| o.id))
                        .collect(),
                    no_longer_hold: self
                        .no_longer_hold
                        .into_iter()
                        .chain(no_longer_hold.into_iter().map(|o| o.id))
                        .collect(),
                    ..self
                }
            }
        }
    }
}

/// A unit on its way to the reviewer: what the prompt shows of it, and the
/// version of its code (fingerprint) it is sent at, so the answer is
/// recorded against that version.
struct UnitToSend {
    prompt: ReviewUnit,
    fingerprint: Fingerprint,
}

/// What happened to one batch.
enum BatchOutcome {
    /// Held back for a later run: the spending limit left no room.
    HeldBack { units: usize },
    /// The call failed, or the user cancelled before it: one line saying so.
    Failed(String),
    /// The reviewer answered. `shown` are the ids of the open findings it
    /// was asked about: an answer on any other id is ignored.
    Answered {
        units: Vec<SentUnit>,
        reply: ReviewReply,
        shown: HashSet<String>,
    },
}

/// What a review call returns: the reply and its errand, or why it failed.
type CallResult = sorcerer_models::Result<Reviewed>;

impl Pass<'_, '_> {
    /// Sends the units not reviewed yet (every unit when `fresh`) to the
    /// reviewer, in batches of at most 600 lines, within the spending
    /// limits; returns what the calls gave.
    ///
    /// Example: three changed functions of 400, 400 and 300 lines make
    /// three calls. If the budget covers two, the third batch is held back:
    /// its unit is not marked reviewed and waits for the next run.
    pub(super) fn send(&mut self, scope: &Scope, fresh: bool) -> ReviewResults {
        let to_send = self.units_not_yet_reviewed(scope, fresh);
        self.log(&units_text(scope, &to_send, fresh));
        let batches = self.batches_of(&to_send);
        self.log(&review_calls_heading_text(batches.is_empty()));
        batches
            .into_iter()
            .enumerate()
            .fold(ReviewResults::default(), |so_far, (n, batch)| {
                self.log(&batch_heading_text(n, &batch));
                let outcome = self.send_batch(scope, batch, &so_far);
                so_far.with(outcome)
            })
    }

    /// The scope's units, less those already reviewed at their current
    /// fingerprint (unless `fresh`).
    fn units_not_yet_reviewed<'s>(&self, scope: &'s Scope, fresh: bool) -> Vec<&'s ScopedUnit> {
        let reviewed = |u: &ScopedUnit| {
            self.working_copy
                .reviewed
                .get(&u.file)
                .is_some_and(|s| s.contains(u.unit.fingerprint.as_str()))
        };
        scope
            .units
            .iter()
            .filter(|u| fresh || !reviewed(u))
            .collect()
    }

    /// The units as the reviewer will read them (each with its own lines of
    /// the file) and the fingerprint each is sent at, grouped into batches
    /// of one call each.
    fn batches_of(&mut self, to_send: &[&ScopedUnit]) -> Vec<Vec<UnitToSend>> {
        let units = to_send
            .iter()
            .map(|u| UnitToSend {
                prompt: ReviewUnit {
                    file: u.file.clone(),
                    name: u.unit.name.clone(),
                    lines: u.unit.span,
                    header: u.header.clone(),
                    text: lines_of(&self.text(&u.file), &u.unit),
                    in_progress: false,
                },
                fingerprint: u.unit.fingerprint,
            })
            .collect();
        batch_by_lines(units, |u| u.prompt.lines)
    }

    /// One batch: held back, failed as cancelled, or reviewed.
    fn send_batch(
        &mut self,
        scope: &Scope,
        batch: Vec<UnitToSend>,
        so_far: &ReviewResults,
    ) -> BatchOutcome {
        if self.must_hold_back(so_far) {
            self.hold_back_for_a_later_run(&batch)
        } else if self.deps.review_cancel.is_cancelled() {
            self.log(CANCELLED_TEXT);
            BatchOutcome::Failed(cancelled_text(&files_of(&batch)))
        } else {
            self.review_one_batch(scope, batch)
        }
    }

    /// Whether the next batch waits for a later run: the spending limits
    /// leave no room for a call's estimated cost. Once one batch is held
    /// back, every later one is too.
    fn must_hold_back(&self, so_far: &ReviewResults) -> bool {
        so_far.capped > 0 || !self.budget.allows(self.review_estimate)
    }

    /// Records the batch's units as held back by a spending limit, so the
    /// next run sends them.
    fn hold_back_for_a_later_run(&mut self, batch: &[UnitToSend]) -> BatchOutcome {
        self.log(&held_back_text(
            self.review_estimate,
            self.budget.money_left(),
        ));
        for u in batch {
            self.working_copy
                .capped
                .entry(u.prompt.file.clone())
                .or_default()
                .insert(u.prompt.name.clone());
        }
        BatchOutcome::HeldBack { units: batch.len() }
    }

    /// One review call on the batch:
    ///
    /// 1. builds the question: the units' code, the diffs, the project's
    ///    rules, and the open findings on these units for the reviewer to
    ///    answer (still holds, or no longer holds);
    /// 2. tells the caller which units are about to be sent, then asks the
    ///    reviewer, under a cost cap for this one call (the review cancel
    ///    can stop it);
    /// 3. charges what the call cost;
    /// 4. a call that failed, or a reply that is not the JSON asked for, is
    ///    a failure; else the reply is the batch's answer.
    fn review_one_batch(&mut self, scope: &Scope, batch: Vec<UnitToSend>) -> BatchOutcome {
        let units: Vec<SentUnit> = batch.iter().map(UnitToSend::sent).collect();
        let ask = self.ask_for(scope, batch.into_iter().map(|u| u.prompt).collect());
        let cap = self.cap_for_one_call();
        self.log(&ask_text(&self.cfg.review_model, cap, &ask));
        self.tell_progress(Progress::Sending(units.clone()));
        let found =
            self.deps
                .reviewer
                .review(&ask, &self.cfg.review_model, cap, self.deps.review_cancel);
        self.log(&reply_text(&found));
        self.budget.charge(self.cost_of_call(&found, cap));
        let files = files_of_prompt(&ask.units);
        let answer = match found {
            Ok(answer) => answer,
            Err(e) => return BatchOutcome::Failed(failed_text(&files, &e.to_string())),
        };
        match parse_review(&answer.reply) {
            Err(why) => {
                self.log(&parse_error_text(&why));
                BatchOutcome::Failed(failed_text(&files, &why))
            }
            Ok(reply) => {
                self.log(&parsed_text(&ask, &reply));
                self.tell_progress(Progress::Reviewed(reply.findings.len()));
                let shown = ask.open.iter().map(|f| f.id.0.clone()).collect();
                BatchOutcome::Answered {
                    units,
                    reply,
                    shown,
                }
            }
        }
    }

    /// The question for one call about the batch: its diffs, the project's
    /// rules (steering), and the open findings on its units to answer.
    fn ask_for(&self, scope: &Scope, batch: Vec<ReviewUnit>) -> ReviewAsk {
        ReviewAsk {
            diff: diff_of(scope, &files_of_prompt(&batch)),
            steering: self.steering.steering_text(),
            // spellbook's slot: what was reported before; always empty in v1
            reported: Vec::new(),
            open: self.open_on(&batch),
            conjure_drops: self.conjure_drops_in(&batch),
            units: batch,
        }
    }

    /// What conjure dropped in the batch's files that the files still lack.
    fn conjure_drops_in(&self, batch: &[ReviewUnit]) -> Vec<ConjureDrop> {
        self.deps
            .settings
            .conjure_drops()
            .into_iter()
            .filter(|drop| batch.iter().any(|unit| unit.file == drop.file))
            .filter(|drop| {
                let text = self.file_texts.get(&drop.file).cloned().or_else(|| {
                    self.fences
                        .allows(&drop.file)
                        .then(|| self.deps.view.text(&drop.file).ok())
                        .flatten()
                });
                text.is_some_and(|text| still_dropped(drop, &text))
            })
            .collect()
    }

    /// The open and unverified findings on the batch's units, for the
    /// reviewer to say whether each still holds.
    fn open_on(&self, batch: &[ReviewUnit]) -> Vec<Finding> {
        self.working_copy
            .findings
            .iter()
            .filter(|f| matches!(f.status, Status::Open | Status::Unverified))
            .filter(|f| {
                batch.iter().any(|u| {
                    u.file == f.place.file && f.place.unit.as_deref() == Some(u.name.as_str())
                })
            })
            .cloned()
            .collect()
    }

    /// The most one call may cost: what the spending limits leave, and never
    /// more than one pass may spend (`[scry] budget_per_pass`). On the
    /// command line nothing else bounds it, and a call that times out is
    /// charged its whole cap.
    ///
    /// Example: the budget has $0.30 left and `budget_per_pass` is $0.25:
    /// the call's cap is $0.25.
    fn cap_for_one_call(&self) -> f64 {
        self.budget
            .money_left()
            .map_or(self.cfg.budget_per_pass, |r| {
                r.min(self.cfg.budget_per_pass)
            })
    }

    /// What a call is charged: its recorded cost (its estimate when the
    /// provider gave no price); a timeout, the cap it ran under (what its
    /// record says); any other failure, nothing.
    fn cost_of_call(&self, found: &CallResult, cap: f64) -> f64 {
        match found {
            Ok(f) => f.errand.cost_usd.unwrap_or(self.review_estimate),
            Err(sorcerer_models::Error::Timeout { .. }) => cap,
            Err(_) => 0.0,
        }
    }
}

impl UnitToSend {
    /// The unit as it was sent: its file, name and fingerprint.
    fn sent(&self) -> SentUnit {
        SentUnit {
            file: self.prompt.file.clone(),
            unit: self.prompt.name.clone(),
            fingerprint: self.fingerprint,
        }
    }
}

/// The unit's own lines of the file's text.
fn lines_of(text: &str, u: &Unit) -> String {
    text.lines()
        .skip(u.span.start.saturating_sub(1) as usize)
        .take((u.span.end + 1).saturating_sub(u.span.start) as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The batch's files, in order, each once.
fn files_of(batch: &[UnitToSend]) -> Vec<PathBuf> {
    first_of_each(batch.iter().map(|u| &u.prompt.file))
}

/// The files of the prompt's units, in order, each once.
fn files_of_prompt(units: &[ReviewUnit]) -> Vec<PathBuf> {
    first_of_each(units.iter().map(|u| &u.file))
}

/// The files in the order first seen, each once.
fn first_of_each<'a>(files: impl Iterator<Item = &'a PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    files
        // `insert` is false for a file already seen, so each file passes once
        .filter(|f| seen.insert((*f).clone()))
        .cloned()
        .collect()
}

/// The files as one comma-separated list, for the log and failure text.
fn file_list(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|f| forward_slashes(f))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The diffs of the files, as the scope holds them.
fn diff_of(scope: &Scope, files: &[PathBuf]) -> String {
    files
        .iter()
        .filter_map(|f| scope.files.iter().find(|s| s.file == *f))
        .map(|s| s.diff.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

// The run log's text for the review calls.

/// The `Units` section: each unit in scope, and whether it is sent.
fn units_text(scope: &Scope, to_send: &[&ScopedUnit], fresh: bool) -> String {
    let mut text = section("Units");
    for u in &scope.units {
        // the same unit of the scope, not just an equal one
        let sending = to_send.iter().any(|s| std::ptr::eq(*s, u));
        let what = if sending && fresh {
            "to send (fresh)"
        } else if sending {
            "to send"
        } else {
            "already reviewed at this fingerprint"
        };
        text.push_str(&format!(
            "- `{}` `{}` ({}-{}, fingerprint {}): {what}\n",
            forward_slashes(&u.file),
            u.unit.name,
            u.unit.span.start,
            u.unit.span.end,
            u.unit.fingerprint,
        ));
    }
    if scope.units.is_empty() {
        text.push_str("- no unit in scope\n");
    }
    text
}

fn review_calls_heading_text(nothing_to_send: bool) -> String {
    let mut text = section("Review calls");
    if nothing_to_send {
        text.push_str("- nothing to send\n");
    }
    text
}

/// A batch's heading (`n` counted from zero, shown from one) and its units.
fn batch_heading_text(n: usize, batch: &[UnitToSend]) -> String {
    let unit_names: Vec<String> = batch
        .iter()
        .map(|u| format!("`{}` `{}`", forward_slashes(&u.prompt.file), u.prompt.name))
        .collect();
    format!(
        "{}- units: {}\n",
        sub(&format!("Batch {}: {}", n + 1, file_list(&files_of(batch)))),
        unit_names.join(", ")
    )
}

fn held_back_text(estimate: f64, money_left: Option<f64>) -> String {
    format!(
        "- capped: the estimate {} does not fit (money left {}); these units wait for a later run\n",
        usd(estimate),
        money_left.map_or("unbounded".into(), usd),
    )
}

const CANCELLED_TEXT: &str = "- cancelled before the call\n";

/// The failure line of a call on the files that failed, and why.
fn failed_text(files: &[PathBuf], why: &str) -> String {
    format!("review failed for {}: {why}", file_list(files))
}

/// The failure line of a batch the user cancelled before its call.
fn cancelled_text(files: &[PathBuf]) -> String {
    format!("review cancelled for {}", file_list(files))
}

/// The call's model and cap, the open findings it is to answer, and its
/// whole prompt.
fn ask_text(model: &str, cap: f64, ask: &ReviewAsk) -> String {
    let open_ids: Vec<&str> = ask.open.iter().map(|f| f.id.0.as_str()).collect();
    format!(
        "- model {}, cap {}\n- open findings to answer: {}\n\nPrompt:\n\n{}",
        model,
        usd(cap),
        if open_ids.is_empty() {
            "none".into()
        } else {
            open_ids.join(", ")
        },
        block(&review_prompt(ask)),
    )
}

/// The call's whole reply and its errand, or why it failed.
fn reply_text(found: &CallResult) -> String {
    match found {
        Ok(f) => format!(
            "Reply:\n\n{}- errand: {}, {:.1} s, {}\n",
            block(&f.reply),
            f.errand.cost_usd.map_or("unpriced".into(), usd),
            f.errand.seconds,
            if f.errand.ok { "ok" } else { "not ok" },
        ),
        Err(e) => format!("- the call failed: {e}\n"),
    }
}

fn parse_error_text(why: &str) -> String {
    format!("- parse error: {why}\n")
}

/// How many findings the reply holds, and its answer on each open finding
/// (an answer on one this call was not shown is ignored).
fn parsed_text(ask: &ReviewAsk, reply: &ReviewReply) -> String {
    let answers: Vec<String> = reply
        .open
        .iter()
        .map(|o| {
            let said = match o.holds {
                Holds::Still => "still holds",
                Holds::NoLonger => "no longer holds",
            };
            let counted = if ask.open.iter().any(|f| f.id.0 == o.id) {
                ""
            } else {
                " (not shown to this call: ignored)"
            };
            format!("{} {said}{counted}", o.id)
        })
        .collect();
    format!(
        "- parsed: {}; open answers: {}\n",
        count(reply.findings.len(), "finding"),
        if answers.is_empty() {
            "none".into()
        } else {
            answers.join(", ")
        }
    )
}
