//! The review calls: the scope's units not yet reviewed, packed, one call a
//! pack while the caps allow, each reply parsed.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::unit::Unit;

use super::{Halt, Progress, Run, Sent};
use crate::domain::finding::{Finding, Status};
use crate::domain::log_text::{block, count, section, sub, usd};
use crate::domain::review_prompt::{
    RawFinding, ReviewAsk, ReviewUnit, pack_units, parse_review, review_prompt,
};
use crate::domain::scope::{Scope, ScopedUnit};

/// What the review calls gave: the units sent to a call that answered, the
/// findings, the open findings said to no longer hold or to still hold, the
/// units a cap held back and the calls that failed.
pub(super) struct Asked {
    pub answered: Vec<Sent>,
    pub raws: Vec<RawFinding>,
    pub no_longer: HashSet<String>,
    pub held: HashSet<String>,
    pub capped: usize,
    pub failures: Vec<String>,
}

impl Run<'_, '_> {
    pub(super) fn send(&mut self, scope: &Scope, fresh: bool) -> std::result::Result<Asked, Halt> {
        // the units to send, packed
        let to_send: Vec<&ScopedUnit> = scope
            .units
            .iter()
            .filter(|u| {
                fresh
                    || !self
                        .mine
                        .reviewed
                        .get(&u.file)
                        .is_some_and(|s| s.contains(u.unit.fingerprint.as_str()))
            })
            .collect();
        let mut units_text = section("Units");
        for u in &scope.units {
            let sending = to_send.iter().any(|s| std::ptr::eq(*s, u));
            let what = if sending && fresh {
                "to send (fresh)"
            } else if sending {
                "to send"
            } else {
                "already reviewed at this fingerprint"
            };
            units_text.push_str(&format!(
                "- `{}` `{}` ({}-{}, fingerprint {}): {what}\n",
                u.file.display(),
                u.unit.name,
                u.unit.span.start,
                u.unit.span.end,
                u.unit.fingerprint,
            ));
        }
        if scope.units.is_empty() {
            units_text.push_str("- no unit in scope\n");
        }
        self.t(&units_text);
        let mut prints: HashMap<(PathBuf, String, u32), Fingerprint> = HashMap::new();
        let mut units = Vec::new();
        for u in &to_send {
            prints.insert(
                (u.file.clone(), u.unit.name.clone(), u.unit.span.start),
                u.unit.fingerprint,
            );
            units.push(ReviewUnit {
                file: u.file.clone(),
                name: u.unit.name.clone(),
                lines: u.unit.span,
                header: u.header.clone(),
                text: lines_of(&self.text(&u.file), &u.unit),
                in_progress: false,
            });
        }

        // the review calls, one a pack, while the caps allow; once one pack
        // is capped every later one is, and its units are recorded for later
        let mut capped = 0;
        let mut failures = Vec::new();
        let mut answered: Vec<Sent> = Vec::new();
        let mut raws: Vec<RawFinding> = Vec::new();
        let mut no_longer: HashSet<String> = HashSet::new();
        let mut held: HashSet<String> = HashSet::new();
        let packs = pack_units(units);
        self.t(&section("Review calls"));
        if packs.is_empty() {
            self.t("- nothing to send\n");
        }
        for (n, pack) in packs.into_iter().enumerate() {
            let files = files_of(&pack);
            let unit_names: Vec<String> = pack
                .iter()
                .map(|u| format!("`{}` `{}`", u.file.display(), u.name))
                .collect();
            self.t(&format!(
                "{}- units: {}\n",
                sub(&format!("Pack {}: {}", n + 1, names(&files))),
                unit_names.join(", ")
            ));
            if capped > 0 || !self.budget.allows(self.review_estimate) {
                self.t(&format!(
                    "- capped: the estimate {} does not fit (room {}); these units wait for a later run\n",
                    usd(self.review_estimate),
                    self.budget.room().map_or("unbounded".into(), usd),
                ));
                capped += pack.len();
                for u in &pack {
                    self.mine
                        .capped
                        .entry(u.file.clone())
                        .or_default()
                        .insert(u.name.clone());
                }
                continue;
            }
            if self.cancel.is_cancelled() {
                self.t("- cancelled before the call\n");
                failures.push(format!("review cancelled for {}", names(&files)));
                continue;
            }
            let ask = ReviewAsk {
                diff: diff_of(scope, &files),
                steering: self.st.text.clone(),
                reported: Vec::new(),
                open: self.open_on(&pack),
                units: pack,
            };
            // one call's cap: what the caps leave, never more than a pass
            // may spend (on the command line nothing else bounds it, and a
            // timeout is charged its cap)
            let cap = self.budget.room().map_or(self.cfg.budget_per_pass, |r| {
                r.min(self.cfg.budget_per_pass)
            });
            let open_ids: Vec<&str> = ask.open.iter().map(|f| f.id.0.as_str()).collect();
            self.t(&format!(
                "- model {}, cap {}\n- open findings to answer: {}\n\nPrompt:\n\n{}",
                self.cfg.review_model,
                usd(cap),
                if open_ids.is_empty() {
                    "none".into()
                } else {
                    open_ids.join(", ")
                },
                block(&review_prompt(&ask)),
            ));
            let found = self
                .deps
                .reviewer
                .review(&ask, &self.cfg.review_model, cap, self.cancel);
            match &found {
                Ok(f) => self.t(&format!(
                    "Reply:\n\n{}- errand: {}, {:.1} s, {}\n",
                    block(&f.reply),
                    f.errand.cost_usd.map_or("unpriced".into(), usd),
                    f.errand.seconds,
                    if f.errand.ok { "ok" } else { "not ok" },
                )),
                Err(e) => self.t(&format!("- the call failed: {e}\n")),
            }
            // an unpriced answer is charged its estimate, a timeout the cap
            // it ran under (what its errand records), any other failure nothing
            self.budget.charge(match &found {
                Ok(f) => f.errand.cost_usd.unwrap_or(self.review_estimate),
                Err(sorcerer_models::Error::Timeout { .. }) => cap,
                Err(_) => 0.0,
            });
            let answered_call = found.is_ok();
            let reply = match found {
                Err(e) => Err(e.to_string()),
                Ok(f) => parse_review(&f.reply),
            };
            match reply {
                Err(why) => {
                    if answered_call {
                        self.t(&format!("- parse error: {why}\n"));
                    }
                    failures.push(format!("review failed for {}: {why}", names(&files)))
                }
                Ok(r) => {
                    let answers: Vec<String> = r
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
                    self.t(&format!(
                        "- parsed: {}; open answers: {}\n",
                        count(r.findings.len(), "finding"),
                        if answers.is_empty() {
                            "none".into()
                        } else {
                            answers.join(", ")
                        }
                    ));
                    (self.deps.progress)(&Progress::Reviewed(r.findings.len()));
                    for u in &ask.units {
                        let key = (u.file.clone(), u.name.clone(), u.lines.start);
                        let fp = prints[&key];
                        answered.push((key.0, key.1, key.2, fp));
                    }
                    // an answer counts only for a finding this call was shown
                    let shown: HashSet<&str> = ask.open.iter().map(|f| f.id.0.as_str()).collect();
                    for o in r.open.into_iter().filter(|o| shown.contains(o.id.as_str())) {
                        if o.holds {
                            held.insert(o.id);
                        } else {
                            no_longer.insert(o.id);
                        }
                    }
                    raws.extend(r.findings);
                }
            }
        }

        Ok(Asked {
            answered,
            raws,
            no_longer,
            held,
            capped,
            failures,
        })
    }

    /// The open and waiting findings on PACK's units, for the review call to answer.
    fn open_on(&self, pack: &[ReviewUnit]) -> Vec<Finding> {
        self.mine
            .findings
            .iter()
            .filter(|f| matches!(f.status, Status::Open | Status::Waiting))
            .filter(|f| {
                pack.iter().any(|u| {
                    u.file == f.place.file && f.place.unit.as_deref() == Some(u.name.as_str())
                })
            })
            .cloned()
            .collect()
    }
}

/// The unit's own lines of TEXT.
fn lines_of(text: &str, u: &Unit) -> String {
    text.lines()
        .skip(u.span.start.saturating_sub(1) as usize)
        .take((u.span.end + 1).saturating_sub(u.span.start) as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The pack's files, in order, each once.
fn files_of(pack: &[ReviewUnit]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    pack.iter()
        .filter(|u| seen.insert(u.file.clone()))
        .map(|u| u.file.clone())
        .collect()
}

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
