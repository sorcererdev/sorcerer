//! The review calls. `send` takes the scope's `units_not_yet_reviewed`,
//! writes them in batches (`batches_of`), and for each batch in turn either
//! holds it back for a later run (`hold_back_for_a_later_run`), fails it as
//! cancelled, or has a call review it (`review_one_batch`: `ask_for` the
//! batch, under `cap_for_one_call`, charged `cost_of_call`, the reply parsed
//! and recorded in `Asked`). The run log's text is built last in the file.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::unit::Unit;

use super::{Halt, Progress, Run, Sent};
use crate::domain::finding::{Finding, Status};
use crate::domain::log_text::{block, count, section, sub, usd};
use crate::domain::review_prompt::{
    RawFinding, ReviewAsk, ReviewReply, ReviewUnit, batch_units, parse_review, review_prompt,
};
use crate::domain::scope::{Scope, ScopedUnit};
use crate::ports::reviewer::Reviewed;

/// What the review calls gave: the units sent to a call that answered, the
/// findings, the open findings said to no longer hold or to still hold, the
/// units a cap held back and the calls that failed.
#[derive(Default)]
pub(super) struct Asked {
    pub answered: Vec<Sent>,
    pub raws: Vec<RawFinding>,
    pub no_longer: HashSet<String>,
    pub held: HashSet<String>,
    pub capped: usize,
    pub failures: Vec<String>,
}

/// The fingerprint each unit was sent at, by its file, name and first line.
type SentFingerprints = HashMap<(PathBuf, String, u32), Fingerprint>;

/// What a review call returns: the reply and its errand, or why it failed.
type CallResult = sorcerer_models::Result<Reviewed>;

impl Run<'_, '_> {
    pub(super) fn send(&mut self, scope: &Scope, fresh: bool) -> std::result::Result<Asked, Halt> {
        let to_send = self.units_not_yet_reviewed(scope, fresh);
        self.t(&units_text(scope, &to_send, fresh));
        let sent = sent_fingerprints(&to_send);
        let batches = self.batches_of(&to_send);
        self.t(&review_calls_heading_text(batches.is_empty()));
        let mut asked = Asked::default();
        for (n, batch) in batches.into_iter().enumerate() {
            self.t(&batch_heading_text(n, &batch));
            if self.must_hold_back(&asked) {
                self.hold_back_for_a_later_run(&batch, &mut asked);
            } else if self.cancel.is_cancelled() {
                self.t("- cancelled before the call\n");
                asked.record_cancelled(&files_of(&batch));
            } else {
                self.review_one_batch(scope, batch, &sent, &mut asked);
            }
        }
        Ok(asked)
    }

    /// The scope's units, less those already reviewed at their fingerprint
    /// (unless FRESH).
    fn units_not_yet_reviewed<'s>(&self, scope: &'s Scope, fresh: bool) -> Vec<&'s ScopedUnit> {
        let reviewed = |u: &ScopedUnit| {
            self.mine
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

    /// TO_SEND as the review call reads them, each with its lines of the
    /// file, in batches of one call each.
    fn batches_of(&mut self, to_send: &[&ScopedUnit]) -> Vec<Vec<ReviewUnit>> {
        let units = to_send
            .iter()
            .map(|u| ReviewUnit {
                file: u.file.clone(),
                name: u.unit.name.clone(),
                lines: u.unit.span,
                header: u.header.clone(),
                text: lines_of(&self.text(&u.file), &u.unit),
                in_progress: false,
            })
            .collect();
        batch_units(units)
    }

    /// Whether the next batch waits for a later run: the caps leave no room
    /// for a call's estimate. Once one batch is held back every later one is.
    fn must_hold_back(&self, asked: &Asked) -> bool {
        asked.capped > 0 || !self.budget.allows(self.review_estimate)
    }

    /// Records BATCH's units as held back by a cap, for a later run.
    fn hold_back_for_a_later_run(&mut self, batch: &[ReviewUnit], asked: &mut Asked) {
        self.t(&held_back_text(self.review_estimate, self.budget.room()));
        asked.capped += batch.len();
        for u in batch {
            self.mine
                .capped
                .entry(u.file.clone())
                .or_default()
                .insert(u.name.clone());
        }
    }

    /// One review call on BATCH: asked, charged, and its reply parsed into
    /// ASKED (or its failure recorded there).
    fn review_one_batch(
        &mut self,
        scope: &Scope,
        batch: Vec<ReviewUnit>,
        sent: &SentFingerprints,
        asked: &mut Asked,
    ) {
        let ask = self.ask_for(scope, batch);
        let cap = self.cap_for_one_call();
        self.t(&ask_text(&self.cfg.review_model, cap, &ask));
        let found = self
            .deps
            .reviewer
            .review(&ask, &self.cfg.review_model, cap, self.cancel);
        self.t(&reply_text(&found));
        self.budget.charge(self.cost_of_call(&found, cap));
        let files = files_of(&ask.units);
        match found.map(|f| parse_review(&f.reply)) {
            Err(e) => asked.record_failure(&files, e.to_string()),
            Ok(Err(why)) => {
                self.t(&parse_error_text(&why));
                asked.record_failure(&files, why);
            }
            Ok(Ok(reply)) => {
                self.t(&parsed_text(&ask, &reply));
                (self.deps.progress)(&Progress::Reviewed(reply.findings.len()));
                asked.record_reply(&ask, reply, sent);
            }
        }
    }

    /// The review call's question about BATCH: its diffs, the steering, and
    /// the open findings on its units to answer.
    fn ask_for(&self, scope: &Scope, batch: Vec<ReviewUnit>) -> ReviewAsk {
        ReviewAsk {
            diff: diff_of(scope, &files_of(&batch)),
            steering: self.st.steering_text(),
            reported: Vec::new(),
            open: self.open_on(&batch),
            units: batch,
        }
    }

    /// The open and waiting findings on BATCH's units, for the review call to answer.
    fn open_on(&self, batch: &[ReviewUnit]) -> Vec<Finding> {
        self.mine
            .findings
            .iter()
            .filter(|f| matches!(f.status, Status::Open | Status::Waiting))
            .filter(|f| {
                batch.iter().any(|u| {
                    u.file == f.place.file && f.place.unit.as_deref() == Some(u.name.as_str())
                })
            })
            .cloned()
            .collect()
    }

    /// One call's cap: what the caps leave, never more than a pass may spend
    /// (on the command line nothing else bounds it, and a timeout is charged
    /// its cap).
    fn cap_for_one_call(&self) -> f64 {
        self.budget.room().map_or(self.cfg.budget_per_pass, |r| {
            r.min(self.cfg.budget_per_pass)
        })
    }

    /// What a call is charged: an unpriced answer its estimate, a timeout the
    /// CAP it ran under (what its errand records), any other failure nothing.
    fn cost_of_call(&self, found: &CallResult, cap: f64) -> f64 {
        match found {
            Ok(f) => f.errand.cost_usd.unwrap_or(self.review_estimate),
            Err(sorcerer_models::Error::Timeout { .. }) => cap,
            Err(_) => 0.0,
        }
    }
}

impl Asked {
    /// Records a call's REPLY to ASK: its units as answered at the
    /// fingerprints they were SENT at, its answers on the open findings it
    /// was shown, and its findings.
    fn record_reply(&mut self, ask: &ReviewAsk, reply: ReviewReply, sent: &SentFingerprints) {
        for u in &ask.units {
            let key = (u.file.clone(), u.name.clone(), u.lines.start);
            let fingerprint = sent[&key];
            self.answered.push((key.0, key.1, key.2, fingerprint));
        }
        // an answer counts only for a finding this call was shown
        let shown: HashSet<&str> = ask.open.iter().map(|f| f.id.0.as_str()).collect();
        for o in reply
            .open
            .into_iter()
            .filter(|o| shown.contains(o.id.as_str()))
        {
            if o.holds {
                self.held.insert(o.id);
            } else {
                self.no_longer.insert(o.id);
            }
        }
        self.raws.extend(reply.findings);
    }

    fn record_failure(&mut self, files: &[PathBuf], why: String) {
        self.failures
            .push(format!("review failed for {}: {why}", names(files)));
    }

    fn record_cancelled(&mut self, files: &[PathBuf]) {
        self.failures
            .push(format!("review cancelled for {}", names(files)));
    }
}

/// The fingerprint each unit of TO_SEND is sent at.
fn sent_fingerprints(to_send: &[&ScopedUnit]) -> SentFingerprints {
    to_send
        .iter()
        .map(|u| {
            let key = (u.file.clone(), u.unit.name.clone(), u.unit.span.start);
            (key, u.unit.fingerprint)
        })
        .collect()
}

/// The unit's own lines of TEXT.
fn lines_of(text: &str, u: &Unit) -> String {
    text.lines()
        .skip(u.span.start.saturating_sub(1) as usize)
        .take((u.span.end + 1).saturating_sub(u.span.start) as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The batch's files, in order, each once.
fn files_of(batch: &[ReviewUnit]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    batch
        .iter()
        .filter(|u| seen.insert(u.file.clone()))
        .map(|u| u.file.clone())
        .collect()
}

/// FILES as one comma-separated list.
fn names(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|f| f.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The diffs of FILES, as the scope holds them.
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
            u.file.display(),
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

/// Batch N's heading (counted from one) and its units.
fn batch_heading_text(n: usize, batch: &[ReviewUnit]) -> String {
    let unit_names: Vec<String> = batch
        .iter()
        .map(|u| format!("`{}` `{}`", u.file.display(), u.name))
        .collect();
    format!(
        "{}- units: {}\n",
        sub(&format!("Batch {}: {}", n + 1, names(&files_of(batch)))),
        unit_names.join(", ")
    )
}

fn held_back_text(estimate: f64, room: Option<f64>) -> String {
    format!(
        "- capped: the estimate {} does not fit (room {}); these units wait for a later run\n",
        usd(estimate),
        room.map_or("unbounded".into(), usd),
    )
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
            let said = if o.holds {
                "still holds"
            } else {
                "no longer holds"
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
