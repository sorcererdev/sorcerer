//! The verification rounds, file by file (`verify_files`, `verify_file`): a
//! file's waiting findings, best first (`waiting_best_first`), through
//! their citations and claim checks until `MAX_SHOWN` are open, within the
//! caps. Each round:
//! 1. takes the candidates that fit if all passed (`Room::split`);
//! 2. checks each one's citations (`check_round_citations`, over
//!    `check_citations`): a failure is
//!    `unsure`, an issue type verified by citations alone opens at once,
//!    the rest have their claim check reserved against the caps or held
//!    back;
//! 3. stores what passed before any claim check starts
//!    (`store_what_passed`, over `Pass::save_working_copy`);
//! 4. runs the round's claim checks side by side
//!    (`claim_checks_side_by_side`) and records each (`record_check`);
//! 5. counts what opened (`Room::count_opened`); a held-back check stops
//!    the rounds.
//!
//! The citation check itself is `citations`. The run log's text is built
//! last in the file.

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

/// What a candidate's citations left it with.
enum Citations {
    /// They failed: the finding is unsure.
    Failed,
    /// They hold and its issue type needs no claim check: it is open.
    OpenWithoutCheck,
    /// They hold; its claim check is reserved.
    ToCheck(Claim),
    /// They hold, but its claim check does not fit the caps: it waits.
    HeldBack,
}

impl Pass<'_, '_> {
    /// The rounds over each file in SCOPE that was not deleted; whether a
    /// claim check was held back by the caps. OLD_WAITING are the findings
    /// that were waiting before this pass.
    pub(super) fn verify_files(
        &mut self,
        scope: &Scope,
        old_waiting: &HashSet<FindingId>,
    ) -> std::result::Result<bool, Halt> {
        let mut checks_capped = false;
        for sf in scope.files.iter().filter(|sf| !sf.deleted) {
            checks_capped |= self.verify_file(&sf.file, old_waiting)?;
        }
        Ok(checks_capped)
    }

    /// The rounds over FILE's waiting findings, best first, while any are
    /// left and the pass is not cancelled. A failed check frees its place
    /// for the next round; one that does not fit the caps leaves its finding
    /// waiting and stops the rounds. Whether a check was held back.
    fn verify_file(
        &mut self,
        file: &Path,
        old_waiting: &HashSet<FindingId>,
    ) -> std::result::Result<bool, Halt> {
        let mut room = Room::of(&self.working_copy, file, self.cfg);
        let mut candidates = self.waiting_best_first(file);
        if !candidates.is_empty() {
            self.log(&verification_heading_text(file));
        }
        let mut round = 0;
        while !candidates.is_empty() && !self.cancel.is_cancelled() {
            round += 1;
            let (this_round, later) =
                room.split(candidates, &self.working_copy, old_waiting, self.cfg);
            self.log(&round_text(
                round,
                &room,
                &this_round,
                &later,
                &self.working_copy,
            ));
            if this_round.is_empty() {
                self.log(NOBODY_FITS_TEXT);
                break;
            }
            let (to_check, held) = self.check_round_citations(&this_round);
            if !to_check.is_empty() {
                self.store_what_passed()?;
            }
            for (id, result) in claim_checks_side_by_side(self.deps.checker, to_check) {
                self.record_check(&id, result);
            }
            room.count_opened(&this_round, &self.working_copy, old_waiting, self.cfg);
            if held {
                return Ok(true);
            }
            candidates = later;
        }
        Ok(false)
    }

    /// Each of ROUND's citations checked (`check_citations`): the claims
    /// to check, reserved within the caps, and whether one was held back.
    fn check_round_citations(&mut self, round: &[FindingId]) -> (Vec<(FindingId, Claim)>, bool) {
        let mut to_check = Vec::new();
        let mut reserved = 0.0;
        let mut held = false;
        for id in round {
            match self.check_citations(id, &mut reserved) {
                Citations::ToCheck(claim) => to_check.push((id.clone(), claim)),
                Citations::HeldBack => held = true,
                Citations::Failed | Citations::OpenWithoutCheck => {}
            }
        }
        (to_check, held)
    }

    /// Writes the store and tells `Stored`, so what passed without a claim
    /// check shows before the round's checks finish.
    fn store_what_passed(&mut self) -> std::result::Result<(), Halt> {
        self.save_working_copy()?;
        self.report(Progress::Stored);
        Ok(())
    }

    /// FILE's waiting findings, the most confident first.
    fn waiting_best_first(&self, file: &Path) -> Vec<FindingId> {
        let mut waiting: Vec<&Finding> = self
            .working_copy
            .findings
            .iter()
            .filter(|f| f.place.file == file && f.status == Status::Waiting)
            .collect();
        waiting.sort_by(|a, b| confidence(b).total_cmp(&confidence(a)));
        waiting.into_iter().map(|f| f.id.clone()).collect()
    }

    /// ID's citations checked: a failure makes it unsure; when they hold,
    /// an issue type that needs no claim check opens, and any other has its
    /// claim check reserved against the caps (RESERVED so far this round),
    /// or held back when it does not fit.
    fn check_citations(&mut self, id: &FindingId, reserved: &mut f64) -> Citations {
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
                Citations::Failed
            }
            Ok(evidence) => {
                self.log(&citations_hold_text(id, &evidence));
                let f = by_id_mut(&mut self.working_copy, id);
                f.evidence = evidence;
                if !needs_claim(f) {
                    f.set_status(Status::Open, now);
                    let issue_type = f.issue_type.clone();
                    self.log(&no_claim_check_text(id, &issue_type));
                    Citations::OpenWithoutCheck
                } else if self.budget.allows(*reserved + self.check_estimate) {
                    *reserved += self.check_estimate;
                    Citations::ToCheck(claim_of(f))
                } else {
                    self.log(&check_held_back_text(id, self.check_estimate));
                    Citations::HeldBack
                }
            }
        }
    }

    /// ID's claim check RESULT: its cost charged, its status from the
    /// verdict, the verdict and what it cited kept. An error leaves the
    /// finding waiting: its place is free next round.
    fn record_check(&mut self, id: &FindingId, result: sorcerer_models::Result<Checked>) {
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

/// A file's room for shown findings: how many are open, and how many of
/// those the pass's own new findings opened.
struct Room {
    open_now: usize,
    new_opened: usize,
}

impl Room {
    /// FILE's room as FINDINGS have it before the rounds.
    fn of(findings: &Findings, file: &Path, cfg: &ScryConfig) -> Room {
        let open_now = findings
            .findings
            .iter()
            .filter(|f| f.place.file == file && f.status == Status::Open && shown(cfg, f))
            .count();
        Room {
            open_now,
            new_opened: 0,
        }
    }

    /// The CANDIDATES that fit if all passed, and the rest. The findings
    /// that were waiting before this pass (OLD_WAITING) take a place only
    /// while the file has room; the pass's own new ones may take up to
    /// `MAX_SHOWN` places.
    fn split(
        &self,
        candidates: Vec<FindingId>,
        findings: &Findings,
        old_waiting: &HashSet<FindingId>,
        cfg: &ScryConfig,
    ) -> (Vec<FindingId>, Vec<FindingId>) {
        let (mut open, mut new) = (self.open_now, self.new_opened);
        let mut fit = Vec::new();
        let mut rest = Vec::new();
        for id in candidates {
            let is_new = !old_waiting.contains(&id);
            if (is_new && new >= MAX_SHOWN) || (!is_new && open >= MAX_SHOWN) {
                rest.push(id);
                continue;
            }
            let place = usize::from(shown(cfg, by_id(findings, &id)));
            open += place;
            if is_new {
                new += place;
            }
            fit.push(id);
        }
        (fit, rest)
    }

    /// Counts the findings of ROUND that are now open and shown.
    fn count_opened(
        &mut self,
        round: &[FindingId],
        findings: &Findings,
        old_waiting: &HashSet<FindingId>,
        cfg: &ScryConfig,
    ) {
        for id in round {
            let f = by_id(findings, id);
            if f.status == Status::Open && shown(cfg, f) {
                self.open_now += 1;
                if !old_waiting.contains(id) {
                    self.new_opened += 1;
                }
            }
        }
    }
}

/// The claim checks of TO_CHECK, run side by side; their results in the
/// order they were started.
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
fn shown(cfg: &ScryConfig, f: &Finding) -> bool {
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

/// A round's heading, the file's room, the candidates THIS_ROUND takes and
/// those LATER.
fn round_text(
    round: usize,
    room: &Room,
    this_round: &[FindingId],
    later: &[FindingId],
    findings: &Findings,
) -> String {
    format!(
        "{}- room: {} open of {} shown in the file; {} of the pass's own opened, of {}\n\
         - candidates: {}\n- left for a later round: {}\n",
        sub(&format!("Round {round}")),
        room.open_now,
        MAX_SHOWN,
        room.new_opened,
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
