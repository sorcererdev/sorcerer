//! Verification rounds, caps and charges (design §6.9, olifant's
//! `Inspect/Pass.hs`): what a call is likely to cost, whether it still fits
//! the caps, and the rounds that take a file's waiting findings through
//! their citations and claim checks, best kept first, until `max_shown`
//! are open.

use std::collections::HashSet;
use std::path::Path;

use chrono::{DateTime, Utc};
use sorcerer_core::fences::Fences;
use sorcerer_core::ledger::Errand;
use sorcerer_core::{Cancel, Result};
use sorcerer_models::checker::{Checked, Citation, Claim, ClaimKind, Verdict};

use crate::config::ScryConfig;
use crate::finding::{Finding, Id, Status, StoredVerdict};
use crate::judge::citations;
use crate::kind::{Verify, kind};
use crate::pass::{Caps, PassDeps, Progress};
use crate::store::ScryStore;

/// A find call on sonnet, a find call on a haiku model, a claim check:
/// what olifant's 1a measured, used until the ledger has priced rows.
pub const FIND_DEFAULT: f64 = 0.04;
pub const FIND_HAIKU_DEFAULT: f64 = 0.005;
pub const CHECK_DEFAULT: f64 = 0.016;

/// What the next find call is likely to cost: the mean of the last 20
/// priced `scry` `find` rows, else $0.04 ($0.005 when MODEL is a haiku).
pub fn find_estimate(rows: &[Errand], model: &str) -> f64 {
    let default = if model.contains("haiku") {
        FIND_HAIKU_DEFAULT
    } else {
        FIND_DEFAULT
    };
    mean_or(rows, "find", default)
}

/// What a claim check is likely to cost: the mean of the last 20 priced
/// `scry` `check` rows, else $0.016.
pub fn check_estimate(rows: &[Errand]) -> f64 {
    mean_or(rows, "check", CHECK_DEFAULT)
}

fn mean_or(rows: &[Errand], verb: &str, default: f64) -> f64 {
    let costs: Vec<f64> = rows
        .iter()
        .rev()
        .filter(|e| e.product == "scry" && e.verb == verb)
        .filter_map(|e| e.cost_usd)
        .take(20)
        .collect();
    if costs.is_empty() {
        default
    } else {
        costs.iter().sum::<f64>() / costs.len() as f64
    }
}

/// A pass's spending against the caps: the pass's own, the local day's
/// (with what scry spent earlier today) and the run's budget.
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    caps: Caps,
    today: f64,
    spent: f64,
}

impl Budget {
    /// SPENT_TODAY is what scry spent earlier in the user's local day.
    pub fn new(caps: &Caps, spent_today: f64) -> Budget {
        Budget {
            caps: caps.clone(),
            today: spent_today,
            spent: 0.0,
        }
    }

    /// Whether a call estimated at E still fits every cap.
    pub fn allows(&self, estimate: f64) -> bool {
        let after = self.spent + estimate;
        let over = |cap: Option<f64>| cap.is_some_and(|c| after > c);
        !(over(self.caps.pass) || self.today + after > self.caps.day || over(self.caps.budget))
    }

    pub fn charge(&mut self, cost: f64) {
        self.spent += cost;
    }

    /// What this pass has spent.
    pub fn spent(&self) -> f64 {
        self.spent
    }

    /// The least room left under any cap, if any cap is finite.
    pub fn room(&self) -> Option<f64> {
        [
            self.caps.pass.map(|c| c - self.spent),
            Some(self.caps.day - self.today - self.spent).filter(|r| r.is_finite()),
            self.caps.budget.map(|c| c - self.spent),
        ]
        .into_iter()
        .flatten()
        .map(|r| r.max(0.0))
        .reduce(f64::min)
    }
}

/// What a paid call is charged: its errands' costs (one with no cost
/// counts zero).
pub fn cost_of(errands: &[Errand]) -> f64 {
    errands.iter().filter_map(|e| e.cost_usd).sum()
}

pub(crate) struct Verified {
    /// A claim check did not fit the caps: its finding waits.
    pub capped: bool,
}

/// What the rounds need from the pass besides the store and budget.
pub(crate) struct Around<'a> {
    pub fences: &'a Fences,
    pub now: DateTime<Utc>,
    /// The findings that were waiting before this pass: they take a place
    /// only while the file has room; the pass's own new ones may take up
    /// to `max_shown` places.
    pub old_waiting: &'a HashSet<Id>,
    pub check_estimate: f64,
    /// Writes the store as the pass has it now.
    pub save: &'a mut dyn FnMut(&ScryStore) -> Result<()>,
}

/// The rounds over FILE's waiting findings, best kept first. Each round
/// takes those that fit if all passed; their citations are checked first
/// (a failure is `unsure`); a kind verified by citations alone opens at
/// once and is stored before any claim check starts; the round's claim
/// checks are reserved against the budget, then run side by side. A
/// failed check frees its place for the next round; one that does not fit
/// the caps leaves its finding waiting and stops the rounds.
pub(crate) fn verify(
    store: &mut ScryStore,
    file: &Path,
    cfg: &ScryConfig,
    budget: &mut Budget,
    deps: &PassDeps,
    cancel: &Cancel,
    around: &mut Around,
) -> Result<Verified> {
    let now = around.now;
    let shown = |f: &Finding| cfg.kinds.contains(&f.kind);
    let mut open_now = store
        .findings
        .iter()
        .filter(|f| f.place.file == file && f.status == Status::Open && shown(f))
        .count();
    let mut new_passed = 0;
    let mut waiting: Vec<&Finding> = store
        .findings
        .iter()
        .filter(|f| f.place.file == file && f.status == Status::Waiting)
        .collect();
    waiting.sort_by(|a, b| {
        let (ka, kb) = (a.scores.keep.unwrap_or(0.0), b.scores.keep.unwrap_or(0.0));
        kb.total_cmp(&ka)
    });
    let mut cands: Vec<Id> = waiting.into_iter().map(|f| f.id.clone()).collect();

    while !cands.is_empty() && !cancel.is_cancelled() {
        // the round: the candidates that would fit if all passed
        let (mut o, mut n) = (open_now, new_passed);
        let mut batch = Vec::new();
        let mut rest = Vec::new();
        for id in cands {
            let f = find(store, &id);
            let is_new = !around.old_waiting.contains(&id);
            let c = usize::from(shown(f));
            if (is_new && n >= cfg.max_shown) || (!is_new && o >= cfg.max_shown) {
                rest.push(id);
            } else {
                o += c;
                if is_new {
                    n += c;
                }
                batch.push(id);
            }
        }
        if batch.is_empty() {
            break;
        }

        // citations first; claim checks reserved within the caps
        let mut to_check: Vec<(Id, Claim)> = Vec::new();
        let mut reserved = 0.0;
        let mut held = false;
        for id in &batch {
            let f = find_mut(store, id);
            let raw: Vec<_> = f
                .evidence
                .iter()
                .map(|e| (e.file.clone(), e.lines))
                .collect();
            match citations(deps.view, around.fences, &raw) {
                Err(why) => {
                    f.verdict = Some(StoredVerdict {
                        verdict: why,
                        why: String::new(),
                    });
                    f.set_status(Status::Unsure, now);
                }
                Ok(ev) => {
                    f.evidence = ev;
                    if !needs_claim(f) {
                        f.set_status(Status::Open, now);
                    } else if budget.allows(reserved + around.check_estimate) {
                        reserved += around.check_estimate;
                        to_check.push((id.clone(), claim_of(f)));
                    } else {
                        held = true;
                    }
                }
            }
        }

        // shown now, before the checks: what passed without one
        if !to_check.is_empty() {
            (around.save)(store)?;
            (deps.progress)(&Progress::Stored);
        }

        // the round's claim checks, side by side
        let checker = deps.checker;
        let results: Vec<(Id, sorcerer_models::Result<Checked>)> = std::thread::scope(|s| {
            let handles: Vec<_> = to_check
                .into_iter()
                .map(|(id, claim)| s.spawn(move || (id, checker(&claim))))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
                .collect()
        });
        for (id, r) in results {
            // an error leaves the finding waiting: its place is free next round
            let Ok(c) = r else { continue };
            let cost = cost_of(&c.errands);
            budget.charge(cost);
            let f = find_mut(store, &id);
            f.cost_usd += cost;
            f.verdict = Some(StoredVerdict {
                verdict: verdict_word(c.verdict).into(),
                why: c.why,
            });
            f.set_status(
                match c.verdict {
                    Verdict::Right => Status::Open,
                    Verdict::Wrong => Status::Refuted,
                    Verdict::CannotTell => Status::Unsure,
                },
                now,
            );
        }

        for id in &batch {
            let f = find(store, id);
            if f.status == Status::Open && shown(f) {
                open_now += 1;
                if !around.old_waiting.contains(id) {
                    new_passed += 1;
                }
            }
        }
        if held {
            return Ok(Verified { capped: true });
        }
        cands = rest;
    }
    Ok(Verified { capped: false })
}

fn needs_claim(f: &Finding) -> bool {
    kind(&f.kind).is_some_and(|k| k.verify == Verify::CitationsThenClaim)
}

/// The finding's statement, pointed at its cited code.
fn claim_of(f: &Finding) -> Claim {
    Claim {
        statement: f.statement.clone(),
        about: f
            .evidence
            .iter()
            .map(|e| Citation {
                file: e.file.clone(),
                lines: e.lines,
                print: e.print.clone(),
            })
            .collect(),
        kind: ClaimKind::Code,
    }
}

fn verdict_word(v: Verdict) -> &'static str {
    match v {
        Verdict::Right => "right",
        Verdict::Wrong => "wrong",
        Verdict::CannotTell => "cannot_tell",
    }
}

fn find<'s>(store: &'s ScryStore, id: &Id) -> &'s Finding {
    store
        .findings
        .iter()
        .find(|f| f.id == *id)
        .expect("a candidate is in the store")
}

fn find_mut<'s>(store: &'s mut ScryStore, id: &Id) -> &'s mut Finding {
    store
        .findings
        .iter_mut()
        .find(|f| f.id == *id)
        .expect("a candidate is in the store")
}
