//! Verifying the findings the review raised, one file at a time.
//!
//! The reviewer's findings start out "waiting": nobody has confirmed them
//! yet. Before scry shows one, it checks it. A file shows at most 3
//! findings, so scry checks the most confident first and stops once the
//! file's 3 slots are full. See `verify_findings_in_file` for the steps.
//!
//! The check that cited lines exist is `citations`. The run log's text is
//! built last in the file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::unit::{Span, unit_at};
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::domain::claim::{Checked, Claim, Verdict};

use super::{Halt, Pass, Progress};
use crate::domain::budget::cost_of;
use crate::domain::config::{MAX_SHOWN, ScryConfig};
use crate::domain::finding::{
    Evidence, Finding, FindingId, Status, StoredCitation, StoredVerdict, status_word,
};
use crate::domain::findings::Findings;
use crate::domain::log_text::{citation, section, sub, usd};
use crate::domain::scope::Scope;
use crate::domain::verification::{claim_of, needs_claim, verdict_word};
use crate::ports::claim_checker::ClaimChecker;

/// How a round ended for a file.
#[derive(PartialEq)]
enum RoundsEnded {
    /// Every waiting finding was checked, or no slot was left.
    AllDone,
    /// A paid check did not fit the budget: that finding waits for the next
    /// run, and the rounds stopped.
    BudgetRanOut,
}

/// What checking a finding's cited lines left it as.
enum AfterCitedLines {
    /// The lines it cites are not there: it is marked unsure.
    Unsure,
    /// The lines are there and it is a convention: it is open, no paid check.
    OpenedWithoutCheck,
    /// The lines are there; it needs this paid claim check, and the money is
    /// set aside.
    NeedsPaidCheck(Claim),
    /// The lines are there, but the budget can't cover its paid check.
    BudgetRanOut,
}

/// One round's findings after their cited lines were checked.
struct RoundChecked {
    /// The paid claim checks to run, one per finding.
    paid_checks: Vec<(FindingId, Claim)>,
    /// Whether a finding's paid check did not fit the budget.
    budget_ran_out: bool,
}

impl Pass<'_, '_> {
    /// Verifies the waiting findings of every file the review looked at
    /// (deleted files are skipped). Returns true when the budget stopped a
    /// paid check in some file, so the caller can say so.
    pub(super) fn verify_findings_per_file(
        &mut self,
        scope: &Scope,
        waiting_before_this_run: &HashSet<FindingId>,
    ) -> std::result::Result<bool, Halt> {
        let mut budget_ran_out = false;
        for sf in scope.files.iter().filter(|sf| !sf.deleted) {
            let ended = self.verify_findings_in_file(&sf.file, waiting_before_this_run)?;
            budget_ran_out |= ended == RoundsEnded::BudgetRanOut;
        }
        Ok(budget_ran_out)
    }

    /// Verifies one file's waiting findings, a few at a time, until the
    /// file's 3 slots are full or nothing is left. Each round:
    ///
    /// 1. picks the most confident waiting findings that could still be
    ///    shown (the rest wait for a later round);
    /// 2. checks that the lines each one cites really exist (free); if not,
    ///    it is marked unsure;
    /// 3. opens a convention right away; a bug needs a paid claim check, or
    ///    keeps waiting when the budget can't cover it;
    /// 4. saves what opened so far, so it shows right away;
    /// 5. runs the paid checks at the same time and records each answer
    ///    (right: open, wrong: refuted, can't tell: unsure);
    /// 6. counts how many are shown now.
    ///
    /// Example: `orders.py` shows 1 finding and has 4 waiting. Round 1 picks
    /// the best 2 (2 free slots); one is confirmed and opens, one is wrong.
    /// Round 2 picks the next one for the last free slot, and so on.
    fn verify_findings_in_file(
        &mut self,
        file: &Path,
        waiting_before_this_run: &HashSet<FindingId>,
    ) -> std::result::Result<RoundsEnded, Halt> {
        let mut slots =
            ShownSlots::for_file(&self.working_copy, file, self.cfg, waiting_before_this_run);
        let mut waiting = self.waiting_most_confident_first(file);
        if !waiting.is_empty() {
            self.log(&verification_heading_text(file));
        }
        let mut round = 0;
        while !waiting.is_empty() && !self.cancel.is_cancelled() {
            round += 1;
            let (this_round, later) = slots.pick_what_fits(waiting, &self.working_copy);
            self.log(&round_text(
                round,
                &slots,
                &this_round,
                &later,
                &self.working_copy,
            ));
            if this_round.is_empty() {
                self.log(NOBODY_FITS_TEXT);
                break;
            }
            let checked = self.check_cited_lines(&this_round);
            if !checked.paid_checks.is_empty() {
                self.show_what_opened_so_far()?;
            }
            self.run_paid_checks(checked.paid_checks);
            slots.count_newly_shown(&this_round, &self.working_copy);
            if checked.budget_ran_out {
                return Ok(RoundsEnded::BudgetRanOut);
            }
            waiting = later;
        }
        Ok(RoundsEnded::AllDone)
    }

    /// The file's waiting findings, the most confident first.
    fn waiting_most_confident_first(&self, file: &Path) -> Vec<FindingId> {
        let mut waiting: Vec<&Finding> = self
            .working_copy
            .findings
            .iter()
            .filter(|f| f.place.file == file && f.status == Status::Waiting)
            .collect();
        waiting.sort_by(|a, b| confidence(b).total_cmp(&confidence(a)));
        waiting.into_iter().map(|f| f.id.clone()).collect()
    }

    /// Checks the cited lines of each finding in the round, setting money
    /// aside for each paid check that fits the budget.
    ///
    /// Example: the budget has $0.03 left and a check is estimated at
    /// $0.016. The first bug gets its check; the second one does not fit,
    /// so it keeps waiting and `budget_ran_out` is true.
    fn check_cited_lines(&mut self, round: &[FindingId]) -> RoundChecked {
        let mut checked = RoundChecked {
            paid_checks: Vec::new(),
            budget_ran_out: false,
        };
        let mut set_aside = 0.0;
        for id in round {
            match self.check_cited_lines_of(id, &mut set_aside) {
                AfterCitedLines::NeedsPaidCheck(claim) => {
                    checked.paid_checks.push((id.clone(), claim));
                }
                AfterCitedLines::BudgetRanOut => checked.budget_ran_out = true,
                AfterCitedLines::Unsure | AfterCitedLines::OpenedWithoutCheck => {}
            }
        }
        checked
    }

    /// Checks one finding's cited lines. When they are there, a convention
    /// opens at once, and a bug's paid check is added to SET_ASIDE (the money
    /// this round has promised so far) if the budget still covers it.
    fn check_cited_lines_of(&mut self, id: &FindingId, set_aside: &mut f64) -> AfterCitedLines {
        let now = self.now;
        let ranges = cited_ranges(by_id(&self.working_copy, id));
        match citations(self.deps.view, self.fences, &ranges) {
            Err(why) => {
                let cited = ranges_as_checked(self.deps.view, self.fences, &ranges);
                self.log(&citations_fail_text(id, &why, &cited));
                let f = by_id_mut(&mut self.working_copy, id);
                f.verdict = Some(StoredVerdict {
                    verdict: why,
                    why: String::new(),
                    cited,
                });
                f.set_status(Status::Unsure, now);
                AfterCitedLines::Unsure
            }
            Ok(evidence) => {
                self.log(&citations_hold_text(id, &evidence));
                let f = by_id_mut(&mut self.working_copy, id);
                f.evidence = evidence;
                if !needs_claim(f) {
                    f.set_status(Status::Open, now);
                    let issue_type = f.issue_type.clone();
                    self.log(&no_claim_check_text(id, &issue_type));
                    AfterCitedLines::OpenedWithoutCheck
                } else if self.budget.allows(*set_aside + self.check_estimate) {
                    *set_aside += self.check_estimate;
                    AfterCitedLines::NeedsPaidCheck(claim_of(f))
                } else {
                    self.log(&check_held_back_text(id, self.check_estimate));
                    AfterCitedLines::BudgetRanOut
                }
            }
        }
    }

    /// Saves the findings as they are now, so what opened without a paid
    /// check shows in the editor before the paid checks finish.
    fn show_what_opened_so_far(&mut self) -> std::result::Result<(), Halt> {
        self.save_working_copy()?;
        self.report(Progress::Stored);
        Ok(())
    }

    /// Runs the round's paid checks at the same time, then records each
    /// answer in the order the checks were started.
    fn run_paid_checks(&mut self, paid_checks: Vec<(FindingId, Claim)>) {
        for (id, result) in claim_checks_side_by_side(self.deps.checker, paid_checks) {
            self.record_paid_check(&id, result);
        }
    }

    /// Records one paid check's answer: what it cost, and the finding's new
    /// status (right: open, wrong: refuted, can't tell: unsure). A check
    /// that failed (the model did not answer) leaves the finding waiting,
    /// so a later round can try it again.
    fn record_paid_check(&mut self, id: &FindingId, result: sorcerer_models::Result<Checked>) {
        let checked = match result {
            Ok(checked) => checked,
            Err(e) => {
                self.log(&check_failed_text(id, &e));
                return;
            }
        };
        let cost = cost_of(&checked.errands);
        self.budget.charge(cost);
        let status = status_after(checked.verdict);
        let cited = stored_citations(&checked);
        let text = check_text(id, &checked, cost, status, &cited);
        let f = by_id_mut(&mut self.working_copy, id);
        f.cost_usd += cost;
        f.set_status(status, self.now);
        f.verdict = Some(StoredVerdict {
            verdict: verdict_word(checked.verdict).into(),
            why: checked.why,
            cited,
        });
        self.log(&text);
    }
}

/// A file's 3 display slots, and how many are taken.
///
/// A file shows at most 3 findings, so it doesn't bury you. A finding left
/// waiting from an earlier run gets a slot only while one is free. A finding
/// this run just discovered may always use up to 3 slots of its own, so new
/// problems are not crowded out by old ones.
///
/// Example: `orders.py` already shows 2 findings, so `shown` is 2. One old
/// waiting finding can still take the last slot. This run's new findings
/// can take up to 3, counted in `shown_new`.
struct ShownSlots<'a> {
    /// Findings the file shows now (only issue types the project shows).
    shown: usize,
    /// Of those, the ones this run discovered.
    shown_new: usize,
    waiting_before_this_run: &'a HashSet<FindingId>,
    cfg: &'a ScryConfig,
}

impl<'a> ShownSlots<'a> {
    /// The file's slots before the first round: its open findings fill them.
    fn for_file(
        findings: &Findings,
        file: &Path,
        cfg: &'a ScryConfig,
        waiting_before_this_run: &'a HashSet<FindingId>,
    ) -> ShownSlots<'a> {
        let shown = findings
            .findings
            .iter()
            .filter(|f| f.place.file == file && f.status == Status::Open && project_shows(cfg, f))
            .count();
        ShownSlots {
            shown,
            shown_new: 0,
            waiting_before_this_run,
            cfg,
        }
    }

    /// Splits WAITING into the findings this round takes (as many as would
    /// still fit if every one of them opened) and those left for later.
    fn pick_what_fits(
        &self,
        waiting: Vec<FindingId>,
        findings: &Findings,
    ) -> (Vec<FindingId>, Vec<FindingId>) {
        let (mut shown, mut shown_new) = (self.shown, self.shown_new);
        let mut this_round = Vec::new();
        let mut later = Vec::new();
        for id in waiting {
            let is_new = self.is_new(&id);
            let slots_taken = if is_new { shown_new } else { shown };
            if slots_taken >= MAX_SHOWN {
                later.push(id);
                continue;
            }
            let takes_a_slot = usize::from(project_shows(self.cfg, by_id(findings, &id)));
            shown += takes_a_slot;
            if is_new {
                shown_new += takes_a_slot;
            }
            this_round.push(id);
        }
        (this_round, later)
    }

    /// Counts the round's findings that opened and are shown.
    fn count_newly_shown(&mut self, round: &[FindingId], findings: &Findings) {
        for id in round {
            let f = by_id(findings, id);
            if f.status == Status::Open && project_shows(self.cfg, f) {
                self.shown += 1;
                if self.is_new(id) {
                    self.shown_new += 1;
                }
            }
        }
    }

    /// Whether this run discovered the finding (it was not waiting before).
    fn is_new(&self, id: &FindingId) -> bool {
        !self.waiting_before_this_run.contains(id)
    }
}

/// Runs the paid claim checks at the same time (one thread each); their
/// answers come back in the order the checks were started.
fn claim_checks_side_by_side(
    checker: &dyn ClaimChecker,
    to_check: Vec<(FindingId, Claim)>,
) -> Vec<(FindingId, sorcerer_models::Result<Checked>)> {
    std::thread::scope(|s| {
        let handles: Vec<_> = to_check
            .into_iter()
            .map(|(id, claim)| s.spawn(move || (id, checker.check(&claim))))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    })
}

/// What a claim check cited, confirmed or not, as the store keeps it.
fn stored_citations(checked: &Checked) -> Vec<StoredCitation> {
    checked
        .cited
        .iter()
        .map(|x| StoredCitation {
            path: x.raw.path.clone(),
            unit: x.raw.unit.clone(),
            line: x.raw.line,
            confirmed: x.confirmed,
            unit_missed: x.unit_missed,
        })
        .collect()
}

/// Each of RANGES as the store keeps a citation, confirmed when it holds.
fn ranges_as_checked(
    view: &dyn CodebaseView,
    fences: &Fences,
    ranges: &[(PathBuf, Span)],
) -> Vec<StoredCitation> {
    ranges
        .iter()
        .map(|(file, lines)| StoredCitation {
            path: file.display().to_string(),
            unit: None,
            line: Some(lines.start),
            confirmed: cites(view, fences, file, *lines),
            unit_missed: false,
        })
        .collect()
}

fn status_after(verdict: Verdict) -> Status {
    match verdict {
        Verdict::Right => Status::Open,
        Verdict::Wrong => Status::Refuted,
        Verdict::CannotTell => Status::Unsure,
    }
}

/// Whether F's issue type is one the project shows.
fn project_shows(cfg: &ScryConfig, f: &Finding) -> bool {
    cfg.issue_types.contains(&f.issue_type)
}

fn confidence(f: &Finding) -> f64 {
    f.scores.confidence.unwrap_or(0.0)
}

fn cited_ranges(f: &Finding) -> Vec<(PathBuf, Span)> {
    f.evidence
        .iter()
        .map(|e| (e.file.clone(), e.lines))
        .collect()
}

fn by_id<'s>(store: &'s Findings, id: &FindingId) -> &'s Finding {
    store
        .findings
        .iter()
        .find(|f| f.id == *id)
        .expect("a candidate is in the store")
}

fn by_id_mut<'s>(store: &'s mut Findings, id: &FindingId) -> &'s mut Finding {
    store
        .findings
        .iter_mut()
        .find(|f| f.id == *id)
        .expect("a candidate is in the store")
}

/// Each cited range checked by its first line: a relative path inside the
/// root, not fenced, readable, the first line within the file (the end
/// line is ignored: a range running past the end is clamped). The prints:
/// the enclosing unit's fingerprint, else seven hex digits of the whole
/// file's FNV-1a. `Err` is the reason the finding is unsure.
pub fn citations(
    view: &dyn CodebaseView,
    fences: &Fences,
    evidence: &[(PathBuf, Span)],
) -> std::result::Result<Vec<Evidence>, String> {
    if evidence.is_empty() {
        return Err("no evidence cited".into());
    }
    evidence
        .iter()
        .map(|(path, lines)| {
            cite(view, fences, path, *lines).ok_or_else(|| "cited code not found".to_string())
        })
        .collect()
}

/// Whether one cited range holds, as [`citations`] checks it.
pub fn cites(view: &dyn CodebaseView, fences: &Fences, path: &Path, lines: Span) -> bool {
    cite(view, fences, path, lines).is_some()
}

fn cite(view: &dyn CodebaseView, fences: &Fences, path: &Path, lines: Span) -> Option<Evidence> {
    if path.is_absolute() || !fences.allows(path) {
        return None;
    }
    let text = view.text(path).ok()?;
    let rows = text.lines().count() as u32;
    if lines.start < 1 || lines.start > rows {
        return None;
    }
    let units = view.units(path).unwrap_or_default();
    let print = match unit_at(&units, lines.start) {
        Some(u) => u.fingerprint.to_string(),
        None => fnv64_hex(text.as_bytes())[..7].to_string(),
    };
    Some(Evidence {
        file: path.to_path_buf(),
        lines,
        print: Some(print),
    })
}

// The run log's text for the rounds.

const NOBODY_FITS_TEXT: &str = "- nobody fits: the rounds end\n";

fn verification_heading_text(file: &Path) -> String {
    section(&format!("Verification: {}", file.display()))
}

/// A round's heading, the file's slots, the findings this round takes and
/// those left for later.
fn round_text(
    round: usize,
    slots: &ShownSlots,
    this_round: &[FindingId],
    later: &[FindingId],
    findings: &Findings,
) -> String {
    format!(
        "{}- room: {} open of {} shown in the file; {} of the pass's own opened, of {}\n\
         - candidates: {}\n- left for a later round: {}\n",
        sub(&format!("Round {round}")),
        slots.shown,
        MAX_SHOWN,
        slots.shown_new,
        MAX_SHOWN,
        listed_text(this_round, findings),
        listed_text(later, findings),
    )
}

/// `00000a (confidence 0.95), …`, or `none`.
fn listed_text(ids: &[FindingId], findings: &Findings) -> String {
    if ids.is_empty() {
        return "none".to_string();
    }
    ids.iter()
        .map(|id| {
            format!(
                "{} (confidence {:.2})",
                id.0,
                confidence(by_id(findings, id))
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn citations_fail_text(id: &FindingId, why: &str, cited: &[StoredCitation]) -> String {
    let each: Vec<String> = cited.iter().map(citation).collect();
    format!(
        "- {}: citations fail: {why}{}{} → unsure\n",
        id.0,
        if each.is_empty() { "" } else { "; " },
        each.join("; ")
    )
}

fn citations_hold_text(id: &FindingId, evidence: &[Evidence]) -> String {
    let each: Vec<String> = evidence
        .iter()
        .map(|e| {
            format!(
                "`{}` {}-{} (print {})",
                e.file.display(),
                e.lines.start,
                e.lines.end,
                e.print.as_deref().unwrap_or("none")
            )
        })
        .collect();
    format!("- {}: citations hold: {}\n", id.0, each.join(", "))
}

fn no_claim_check_text(id: &FindingId, issue_type: &str) -> String {
    format!("- {}: a {issue_type} needs no claim check → open\n", id.0)
}

fn check_held_back_text(id: &FindingId, estimate: f64) -> String {
    format!(
        "- {}: claim check held back: {} more does not fit the caps; it waits\n",
        id.0,
        usd(estimate)
    )
}

fn check_failed_text(id: &FindingId, error: &sorcerer_models::Error) -> String {
    format!(
        "- {} claim check failed: {error} → it stays waiting\n",
        id.0
    )
}

/// A claim check's models, verdict, cost and the STATUS it leaves, then
/// what it CITED.
fn check_text(
    id: &FindingId,
    checked: &Checked,
    cost: f64,
    status: Status,
    cited: &[StoredCitation],
) -> String {
    let models: Vec<&str> = checked.errands.iter().map(|e| e.model.as_str()).collect();
    let mut text = format!(
        "- {} claim check: models: {}; escalated: {}; verdict {}: {}; cost {} → {}\n",
        id.0,
        if models.is_empty() {
            "none recorded".into()
        } else {
            models.join(", ")
        },
        if checked.escalated { "yes" } else { "no" },
        verdict_word(checked.verdict),
        checked.why,
        usd(cost),
        status_word(status),
    );
    if cited.is_empty() {
        text.push_str("  - cited nothing\n");
    }
    for x in cited {
        text.push_str(&format!("  - cited {}\n", citation(x)));
    }
    text
}
