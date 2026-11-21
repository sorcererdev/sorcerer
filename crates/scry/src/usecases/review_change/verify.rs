//! Verifying the findings the review raised, one file at a time.
//!
//! A finding the reviewer raises is unverified: nobody has checked it yet.
//! Before scry displays one, it
//! checks it. A file displays at most 3 findings, so scry checks the most
//! confident first (`PrioritizedFindings`) and stops once the file's 3
//! display slots (`DisplaySlots`) are full. See `verify_findings_in_file`
//! for the steps.
//!
//! The check that cited lines exist is `citations`. The run log's text is
//! built last in the file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::{Span, unit_at};
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::domain::claim::{Checked, CitationCheck, Claim, Escalation, Verdict};

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
    /// Every unverified finding was checked, or no display slot was left.
    AllDone,
    /// A paid check did not fit the budget: that finding stays unverified
    /// until the next run, and the rounds stopped.
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
#[derive(Default)]
struct RoundChecked {
    /// The paid claim checks to run, one per finding.
    paid_checks: Vec<(FindingId, Claim)>,
    /// The money set aside so far for those checks.
    set_aside: f64,
    /// Whether a finding's paid check did not fit the budget.
    budget_ran_out: bool,
}

impl RoundChecked {
    /// This round's record with one more finding's outcome added. A paid
    /// check also sets its estimated cost aside.
    fn with(self, id: &FindingId, after: AfterCitedLines, check_estimate: f64) -> RoundChecked {
        match after {
            AfterCitedLines::NeedsPaidCheck(claim) => RoundChecked {
                paid_checks: [self.paid_checks, vec![(id.clone(), claim)]].concat(),
                set_aside: self.set_aside + check_estimate,
                ..self
            },
            AfterCitedLines::BudgetRanOut => RoundChecked {
                budget_ran_out: true,
                ..self
            },
            AfterCitedLines::Unsure | AfterCitedLines::OpenedWithoutCheck => self,
        }
    }
}

/// A file's unverified findings, most confident first.
///
/// The order is the point: the findings most likely to be real are checked
/// first, so the file's 3 display slots go to them. The only way to make one
/// is `unverified_in`, which sorts, so a `PrioritizedFindings` is always in
/// order.
///
/// Example: `orders.py` has unverified findings at confidence 0.7, 0.95 and
/// 0.8; as `PrioritizedFindings` they are 0.95, 0.8, 0.7.
struct PrioritizedFindings(Vec<FindingId>);

impl PrioritizedFindings {
    /// The file's unverified findings, sorted most confident first.
    fn unverified_in(findings: &Findings, file: &Path) -> PrioritizedFindings {
        let mut unverified: Vec<&Finding> = findings
            .findings
            .iter()
            .filter(|f| f.place.file == file && f.status == Status::Unverified)
            .collect();
        unverified.sort_by(|a, b| confidence(b).total_cmp(&confidence(a)));
        PrioritizedFindings(unverified.into_iter().map(|f| f.id.clone()).collect())
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn ids(&self) -> &[FindingId] {
        &self.0
    }
}

/// The findings one round checks: picked from `PrioritizedFindings` because
/// they could all be displayed if they all turn out true. Only the ones the
/// checks confirm are displayed.
struct FindingsToVerify(Vec<FindingId>);

impl FindingsToVerify {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn ids(&self) -> &[FindingId] {
        &self.0
    }
}

impl Pass<'_, '_> {
    /// Verifies the unverified findings of every file the review looked at
    /// (deleted files are skipped). Returns true when the budget stopped a
    /// paid check in some file, so the caller can say so.
    pub(super) fn verify_findings_per_file(
        &mut self,
        scope: &Scope,
        unverified_from_earlier_runs: &HashSet<FindingId>,
    ) -> std::result::Result<bool, Halt> {
        scope
            .files
            .iter()
            .filter(|sf| !sf.deleted)
            .try_fold(false, |budget_ran_out, sf| {
                let ended = self.verify_findings_in_file(&sf.file, unverified_from_earlier_runs)?;
                Ok(budget_ran_out || ended == RoundsEnded::BudgetRanOut)
            })
    }

    /// Verifies one file's unverified findings, a few at a time, until its 3
    /// display slots are full or nothing is left. Each round:
    ///
    /// 1. picks, most confident first, the findings that could all still be
    ///    displayed if they all turn out true (the rest wait for a later
    ///    round);
    /// 2. checks that the lines each one cites really exist (free); if not,
    ///    it is marked unsure;
    /// 3. opens a convention right away; a bug needs a paid claim check, or
    ///    stays unverified when the budget can't cover it;
    /// 4. saves what opened so far, so it is displayed right away;
    /// 5. runs the paid checks at the same time and records each answer
    ///    (right: open, wrong: refuted, can't tell: unsure);
    /// 6. counts the display slots now taken, and goes again with the rest.
    ///
    /// Example: `orders.py` displays 1 finding and has 4 unverified. Round 1
    /// picks the best 2 (2 free slots); one is confirmed and opens, one is
    /// wrong. Round 2 picks the next one for the last free slot, and so on.
    fn verify_findings_in_file(
        &mut self,
        file: &Path,
        unverified_from_earlier_runs: &HashSet<FindingId>,
    ) -> std::result::Result<RoundsEnded, Halt> {
        let slots = DisplaySlots::for_file(
            &self.working_copy,
            file,
            self.cfg,
            unverified_from_earlier_runs,
        );
        let prioritized = PrioritizedFindings::unverified_in(&self.working_copy, file);
        if !prioritized.is_empty() {
            self.log(&verification_heading_text(file));
        }
        self.verify_round(1, slots, prioritized)
    }

    /// One round over the file's prioritized findings, then the next round
    /// with the ones left over, until none are left, none fit, the budget
    /// runs out or the user cancels.
    fn verify_round(
        &mut self,
        round: usize,
        slots: DisplaySlots,
        prioritized: PrioritizedFindings,
    ) -> std::result::Result<RoundsEnded, Halt> {
        if prioritized.is_empty() || self.deps.cancel.is_cancelled() {
            return Ok(RoundsEnded::AllDone);
        }
        let (to_verify, later) = slots.pick_for_this_round(prioritized, &self.working_copy);
        self.log(&round_text(
            round,
            &slots,
            to_verify.ids(),
            later.ids(),
            &self.working_copy,
        ));
        if to_verify.is_empty() {
            self.log(NOBODY_FITS_TEXT);
            return Ok(RoundsEnded::AllDone);
        }
        let checked = self.check_cited_lines(&to_verify);
        if !checked.paid_checks.is_empty() {
            self.save_before_paid_checks()?;
        }
        self.run_paid_checks(checked.paid_checks);
        if checked.budget_ran_out {
            return Ok(RoundsEnded::BudgetRanOut);
        }
        let slots = slots.after_round(&to_verify, &self.working_copy);
        self.verify_round(round + 1, slots, later)
    }

    /// Checks the cited lines of each finding in the round, setting money
    /// aside for each paid check that fits the budget.
    ///
    /// Example: the budget has $0.03 left and a check is estimated at
    /// $0.016. The first bug gets its check; the second one does not fit,
    /// so it stays unverified and `budget_ran_out` is true.
    fn check_cited_lines(&mut self, to_verify: &FindingsToVerify) -> RoundChecked {
        to_verify
            .ids()
            .iter()
            .fold(RoundChecked::default(), |checked, id| {
                let after = self.check_cited_lines_of(id, checked.set_aside);
                checked.with(id, after, self.check_estimate)
            })
    }

    /// Checks one finding's cited lines. When they are there, a convention
    /// opens at once, and a bug needs a paid check if the budget still
    /// covers it on top of the money this round has already set aside.
    fn check_cited_lines_of(&mut self, id: &FindingId, set_aside: f64) -> AfterCitedLines {
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
                } else if self.budget.allows(set_aside + self.check_estimate) {
                    AfterCitedLines::NeedsPaidCheck(claim_of(f))
                } else {
                    self.log(&check_held_back_text(id, self.check_estimate));
                    AfterCitedLines::BudgetRanOut
                }
            }
        }
    }

    /// Saves the findings to the store before the paid checks start, which
    /// can take a minute. A convention that opened without a paid check is
    /// then in the store at once, so `scry list` in another terminal (and,
    /// later, the editor, told by `Progress::Stored`) can display it while
    /// the checks still run.
    fn save_before_paid_checks(&mut self) -> std::result::Result<(), Halt> {
        self.save_working_copy()?;
        self.tell_progress(Progress::Stored);
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
    /// that failed (the model did not answer) leaves the finding unverified,
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
/// A file displays at most 3 findings, so it doesn't bury you. A finding
/// still unverified from an earlier run gets a slot only while one is free.
/// A finding this run just discovered may always use up to 3 slots of its
/// own, so new problems are not crowded out by old ones.
///
/// Example: `orders.py` already displays 2 findings, so `taken` is 2. One
/// finding from an earlier run can still take the last slot. This run's new
/// findings can take up to 3, counted in `taken_by_new`.
#[derive(Clone, Copy)]
struct DisplaySlots<'a> {
    /// Findings the file displays now (only issue types the project shows).
    taken: usize,
    /// Of those, the ones this run discovered.
    taken_by_new: usize,
    unverified_from_earlier_runs: &'a HashSet<FindingId>,
    cfg: &'a ScryConfig,
}

impl<'a> DisplaySlots<'a> {
    /// The file's slots before the first round: its open findings take them.
    fn for_file(
        findings: &Findings,
        file: &Path,
        cfg: &'a ScryConfig,
        unverified_from_earlier_runs: &'a HashSet<FindingId>,
    ) -> DisplaySlots<'a> {
        let taken = findings
            .findings
            .iter()
            .filter(|f| f.place.file == file && f.status == Status::Open && project_shows(cfg, f))
            .count();
        DisplaySlots {
            taken,
            taken_by_new: 0,
            unverified_from_earlier_runs,
            cfg,
        }
    }

    /// Picks this round's findings to verify. Going down the prioritized
    /// findings in order, each is picked while a slot would still be free if
    /// every finding picked so far turned out true. The rest stay prioritized
    /// for a later round, where a slot freed by a pick that turned out wrong
    /// may take them.
    ///
    /// Example: `orders.py` displays 2 findings, so 1 slot is free. The
    /// prioritized findings are one from an earlier run (0.9) and two new
    /// ones (0.8, 0.7). The earlier one takes the free slot; both new ones
    /// are picked too, since this run's own findings may use up to 3 slots.
    /// If the earlier one turns out wrong, its slot is free again next round.
    fn pick_for_this_round(
        &self,
        prioritized: PrioritizedFindings,
        findings: &Findings,
    ) -> (FindingsToVerify, PrioritizedFindings) {
        let mut if_all_turn_out_true = *self;
        let (to_verify, later) = prioritized.0.into_iter().partition(|id| {
            let has_room = if_all_turn_out_true.has_room_for(id);
            if has_room {
                if_all_turn_out_true = if_all_turn_out_true.taken_by(by_id(findings, id));
            }
            has_room
        });
        // `partition` keeps the order, so what is left is still prioritized
        (FindingsToVerify(to_verify), PrioritizedFindings(later))
    }

    /// Whether a slot is free for the finding: one from an earlier run needs
    /// one of the file's 3; one this run discovered may use up to 3 of its
    /// own.
    fn has_room_for(&self, id: &FindingId) -> bool {
        let taken = if self.is_new(id) {
            self.taken_by_new
        } else {
            self.taken
        };
        taken < MAX_SHOWN
    }

    /// These slots with the finding displayed in one of them (an issue type
    /// the project does not show takes no slot).
    fn taken_by(self, f: &Finding) -> DisplaySlots<'a> {
        if !project_shows(self.cfg, f) {
            return self;
        }
        DisplaySlots {
            taken: self.taken + 1,
            taken_by_new: self.taken_by_new + usize::from(self.is_new(&f.id)),
            ..self
        }
    }

    /// These slots after a round: each finding the checks opened takes one.
    fn after_round(self, verified: &FindingsToVerify, findings: &Findings) -> DisplaySlots<'a> {
        verified
            .ids()
            .iter()
            .map(|id| by_id(findings, id))
            .filter(|f| f.status == Status::Open)
            .fold(self, DisplaySlots::taken_by)
    }

    /// Whether this run discovered the finding (it was not unverified
    /// before the run).
    fn is_new(&self, id: &FindingId) -> bool {
        !self.unverified_from_earlier_runs.contains(id)
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
            confirmed: x.check != CitationCheck::NotConfirmed,
            unit_missed: x.check == CitationCheck::ConfirmedOnTheLineOnly,
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
            path: forward_slashes(file),
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
    section(&format!("Verification: {}", forward_slashes(file)))
}

/// A round's heading, the file's slots, the findings this round takes and
/// those left for later.
fn round_text(
    round: usize,
    slots: &DisplaySlots,
    this_round: &[FindingId],
    later: &[FindingId],
    findings: &Findings,
) -> String {
    format!(
        "{}- room: {} open of {} shown in the file; {} of the pass's own opened, of {}\n\
         - candidates: {}\n- left for a later round: {}\n",
        sub(&format!("Round {round}")),
        slots.taken,
        MAX_SHOWN,
        slots.taken_by_new,
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
                forward_slashes(&e.file),
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
        "- {}: claim check held back: {} more does not fit the caps; it stays unverified\n",
        id.0,
        usd(estimate)
    )
}

fn check_failed_text(id: &FindingId, error: &sorcerer_models::Error) -> String {
    format!(
        "- {} claim check failed: {error} → it stays unverified\n",
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
        match checked.escalation {
            Escalation::No => "no",
            Escalation::ToTheStrongModel => "yes",
        },
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
