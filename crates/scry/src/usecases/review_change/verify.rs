//! The verification rounds, file by file: a file's waiting findings, best
//! first, through their citations and claim checks until `MAX_SHOWN` are
//! open, within the caps; and the citation check itself.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::unit::{Span, unit_at};
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Cancel, Result};
use sorcerer_models::domain::claim::{Checked, Claim, Verdict};

use super::{Halt, Progress, ReviewDeps, Run};
use crate::domain::budget::{Budget, cost_of};
use crate::domain::config::{MAX_SHOWN, ScryConfig};
use crate::domain::finding::{
    Evidence, Finding, FindingId, Status, StoredCitation, StoredVerdict, status_word,
};
use crate::domain::findings::{Findings, merge_into, renamed};
use crate::domain::log_text::{citation, section, sub, usd};
use crate::domain::verification::{claim_of, needs_claim, verdict_word};

impl Run<'_, '_> {
    /// The rounds over each file in SCOPE that was not deleted; whether a
    /// claim check was held back by the caps. OLD_WAITING are the findings
    /// that were waiting before this pass.
    pub(super) fn verify_files(
        &mut self,
        scope: &crate::domain::scope::Scope,
        old_waiting: &HashSet<FindingId>,
    ) -> std::result::Result<bool, Halt> {
        let mut checks_capped = false;
        for sf in scope.files.iter().filter(|sf| !sf.deleted) {
            let Run {
                mine,
                written,
                disk,
                budget,
                cfg,
                deps,
                cancel,
                fences,
                now,
                check_estimate,
                renames,
                ..
            } = self;
            let mut save = |s: &Findings| -> Result<()> {
                disk.update(&mut |current| *current = merge_into(written, s, current, renames))?;
                *written = renamed(s, renames);
                Ok(())
            };
            let mut around = Around {
                fences,
                now: *now,
                old_waiting,
                check_estimate: *check_estimate,
                save: &mut save,
            };
            let v = verify(mine, &sf.file, cfg, budget, deps, cancel, &mut around)?;
            checks_capped |= v.capped;
        }

        Ok(checks_capped)
    }
}

struct Verified {
    /// A claim check did not fit the caps: its finding waits.
    pub capped: bool,
}

/// What the rounds need from the pass besides the store and budget.
struct Around<'a> {
    pub fences: &'a Fences,
    pub now: DateTime<Utc>,
    /// The findings that were waiting before this pass: they take a place
    /// only while the file has room; the pass's own new ones may take up
    /// to `MAX_SHOWN` places.
    pub old_waiting: &'a HashSet<FindingId>,
    pub check_estimate: f64,
    /// Writes the store as the pass has it now.
    pub save: &'a mut dyn FnMut(&Findings) -> Result<()>,
}

/// The rounds over FILE's waiting findings, best kept first. Each round
/// takes those that fit if all passed; their citations are checked first
/// (a failure is `unsure`); an issue type verified by citations alone opens at
/// once and is stored before any claim check starts; the round's claim
/// checks are reserved against the budget, then run side by side. A
/// failed check frees its place for the next round; one that does not fit
/// the caps leaves its finding waiting and stops the rounds.
#[expect(clippy::too_many_lines, reason = "readability stage 2")]
fn verify(
    store: &mut Findings,
    file: &Path,
    cfg: &ScryConfig,
    budget: &mut Budget,
    deps: &ReviewDeps,
    cancel: &Cancel,
    around: &mut Around,
) -> Result<Verified> {
    let now = around.now;
    let shown = |f: &Finding| cfg.issue_types.contains(&f.issue_type);
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
        let (ka, kb) = (
            a.scores.confidence.unwrap_or(0.0),
            b.scores.confidence.unwrap_or(0.0),
        );
        kb.total_cmp(&ka)
    });
    let mut cands: Vec<FindingId> = waiting.into_iter().map(|f| f.id.clone()).collect();
    let log = |s: &str| deps.log.put(s);
    if !cands.is_empty() {
        log(&section(&format!("Verification: {}", file.display())));
    }
    let mut round = 0;

    while !cands.is_empty() && !cancel.is_cancelled() {
        round += 1;
        // the round: the candidates that would fit if all passed
        let (mut o, mut n) = (open_now, new_passed);
        let mut batch = Vec::new();
        let mut rest = Vec::new();
        for id in cands {
            let f = by_id(store, &id);
            let is_new = !around.old_waiting.contains(&id);
            let c = usize::from(shown(f));
            if (is_new && n >= MAX_SHOWN) || (!is_new && o >= MAX_SHOWN) {
                rest.push(id);
            } else {
                o += c;
                if is_new {
                    n += c;
                }
                batch.push(id);
            }
        }
        let confidence_of = |id: &FindingId| {
            let f = by_id(store, id);
            format!(
                "{} (confidence {:.2})",
                id.0,
                f.scores.confidence.unwrap_or(0.0)
            )
        };
        let listed = |ids: &[FindingId]| {
            if ids.is_empty() {
                "none".to_string()
            } else {
                ids.iter().map(confidence_of).collect::<Vec<_>>().join(", ")
            }
        };
        log(&format!(
            "{}- room: {open_now} open of {} shown in the file; {new_passed} of the pass's own opened, of {}\n\
             - candidates: {}\n- left for a later round: {}\n",
            sub(&format!("Round {round}")),
            MAX_SHOWN,
            MAX_SHOWN,
            listed(&batch),
            listed(&rest),
        ));
        if batch.is_empty() {
            log("- nobody fits: the rounds end\n");
            break;
        }

        // citations first; claim checks reserved within the caps
        let mut to_check: Vec<(FindingId, Claim)> = Vec::new();
        let mut reserved = 0.0;
        let mut held = false;
        for id in &batch {
            let f = by_id_mut(store, id);
            let raw: Vec<_> = f
                .evidence
                .iter()
                .map(|e| (e.file.clone(), e.lines))
                .collect();
            match citations(deps.view, around.fences, &raw) {
                Err(why) => {
                    let cited: Vec<StoredCitation> = raw
                        .iter()
                        .map(|(file, lines)| StoredCitation {
                            path: file.display().to_string(),
                            unit: None,
                            line: Some(lines.start),
                            confirmed: cites(deps.view, around.fences, file, *lines),
                            unit_missed: false,
                        })
                        .collect();
                    let each: Vec<String> = cited.iter().map(citation).collect();
                    log(&format!(
                        "- {}: citations fail: {why}{}{} → unsure\n",
                        id.0,
                        if each.is_empty() { "" } else { "; " },
                        each.join("; ")
                    ));
                    f.verdict = Some(StoredVerdict {
                        verdict: why,
                        why: String::new(),
                        cited,
                    });
                    f.set_status(Status::Unsure, now);
                }
                Ok(ev) => {
                    let each: Vec<String> = ev
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
                    log(&format!(
                        "- {}: citations hold: {}\n",
                        id.0,
                        each.join(", ")
                    ));
                    f.evidence = ev;
                    if !needs_claim(f) {
                        f.set_status(Status::Open, now);
                        log(&format!(
                            "- {}: a {} needs no claim check → open\n",
                            id.0, f.issue_type
                        ));
                    } else if budget.allows(reserved + around.check_estimate) {
                        reserved += around.check_estimate;
                        to_check.push((id.clone(), claim_of(f)));
                    } else {
                        log(&format!(
                            "- {}: claim check held back: {} more does not fit the caps; it waits\n",
                            id.0,
                            usd(around.check_estimate)
                        ));
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
        let results: Vec<(FindingId, sorcerer_models::Result<Checked>)> = std::thread::scope(|s| {
            let handles: Vec<_> = to_check
                .into_iter()
                .map(|(id, claim)| s.spawn(move || (id, checker.check(&claim))))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
                .collect()
        });
        for (id, r) in results {
            // an error leaves the finding waiting: its place is free next round
            let c = match r {
                Ok(c) => c,
                Err(e) => {
                    log(&format!(
                        "- {} claim check failed: {e} → it stays waiting\n",
                        id.0
                    ));
                    continue;
                }
            };
            let cost = cost_of(&c.errands);
            budget.charge(cost);
            let cited: Vec<StoredCitation> = c
                .cited
                .iter()
                .map(|x| StoredCitation {
                    path: x.raw.path.clone(),
                    unit: x.raw.unit.clone(),
                    line: x.raw.line,
                    confirmed: x.confirmed,
                    unit_missed: x.unit_missed,
                })
                .collect();
            let models: Vec<&str> = c.errands.iter().map(|e| e.model.as_str()).collect();
            let f = by_id_mut(store, &id);
            f.cost_usd += cost;
            f.set_status(
                match c.verdict {
                    Verdict::Right => Status::Open,
                    Verdict::Wrong => Status::Refuted,
                    Verdict::CannotTell => Status::Unsure,
                },
                now,
            );
            let mut text = format!(
                "- {} claim check: models: {}; escalated: {}; verdict {}: {}; cost {} → {}\n",
                id.0,
                if models.is_empty() {
                    "none recorded".into()
                } else {
                    models.join(", ")
                },
                if c.escalated { "yes" } else { "no" },
                verdict_word(c.verdict),
                c.why,
                usd(cost),
                status_word(f.status),
            );
            if cited.is_empty() {
                text.push_str("  - cited nothing\n");
            }
            for x in &cited {
                text.push_str(&format!("  - cited {}\n", citation(x)));
            }
            log(&text);
            f.verdict = Some(StoredVerdict {
                verdict: verdict_word(c.verdict).into(),
                why: c.why,
                cited,
            });
        }

        for id in &batch {
            let f = by_id(store, id);
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
