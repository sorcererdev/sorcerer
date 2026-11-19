//! The pass (design §6.4, olifant's `Inspect/Pass.hs`): the scope's units
//! not yet reviewed go to the review call in packs; each finding is placed,
//! matched against the store, filtered by Jev's nitpick question and
//! remembered; then each file's waiting findings go through the
//! verification rounds (`rounds`), within the caps.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, Local, Utc};
use sorcerer_core::adapters::json_store::{Snapshot, Store};
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::cost::spent_in;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Unit;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Cancel, Error, Result};
use sorcerer_models::domain::claim::{Checked, Claim};
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Weighed};
use sorcerer_models::ports::decider::Decider;

use crate::config::{ScryConfig, scry_config};
use crate::finding::{Evidence, Finding, FindingId, Place, Scores, Status};
use crate::judge::{
    Match, SAME_THRESHOLD, asked_on, candidates, citations, cited_code, confidence_of,
    finding_text, match_stored, nitpick, place, same_problem,
};
use crate::log::{Trace, block, count, section, sub, usd};
use crate::review::{
    RawFinding, ReviewAsk, ReviewUnit, Reviewer, pack_units, parse_review, review_prompt,
};
use crate::rounds::{Around, Budget, check_estimate, review_estimate, verify};
use crate::scope::{Scope, ScopedUnit};
use crate::steering::{Steering, steering};
use crate::store::{ScryStore, store};

/// The time, and the user's offset (a day's cap is a local day).
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn offset(&self) -> FixedOffset;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn offset(&self) -> FixedOffset {
        *Local::now().offset()
    }
}

/// The claim checker, as the pass calls it.
pub type Checker<'a> = &'a (dyn Fn(&Claim) -> sorcerer_models::Result<Checked> + Sync);

/// What a pass is told as it goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// A review call answered with this many findings.
    Reviewed(usize),
    /// The store was written.
    Stored,
}

pub struct PassDeps<'a> {
    pub view: &'a dyn CodebaseView,
    pub reviewer: &'a dyn Reviewer,
    pub decider: &'a dyn Decider,
    pub checker: Checker<'a>,
    pub clock: &'a dyn Clock,
    pub progress: &'a (dyn Fn(&Progress) + Sync),
    /// The run log: each step's inputs and decisions, as Markdown
    /// (`log::quiet` when nobody reads it). Called on the pass's thread only.
    pub trace: Trace<'a>,
}

/// Spend caps in USD: a pass's, the local day's, and a run's budget.
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    pub pass: Option<f64>,
    pub day: f64,
    pub budget: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Skipped(String),
    Ran(Report),
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

/// One pass over SCOPE. With FRESH, units already reviewed are sent again.
/// Holds `state/scry/pass.lock` throughout; writes the store before a
/// round's claim checks start and at the end, each time laying its changes
/// over the store as it is then. CAPS hold the review calls and claim checks
/// back, the day's cap counting what scry spent earlier in the local day.
pub fn pass(
    project: &Project,
    scope: &Scope,
    caps: &Caps,
    fresh: bool,
    deps: &PassDeps,
    cancel: &Cancel,
) -> Result<Outcome> {
    let cfg = scry_config(project)?;
    if !cfg.enabled {
        return Ok(Outcome::Skipped(
            "scry is off ([scry] enabled = false)".into(),
        ));
    }
    let st = steering(project)?;
    let fences = Fences::for_project(project)?;
    let dir = project.state_dir()?.join("scry");
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    let lock_path = dir.join("pass.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| Error::io(&lock_path, e))?;
    lock.lock().map_err(|e| Error::io(&lock_path, e))?;

    let findings = store(project)?;
    let base = match findings.read()? {
        Snapshot::Absent => ScryStore::default(),
        Snapshot::Current(s) => s,
        Snapshot::Unreadable(why) => {
            return Err(Error::StoreUnreadable {
                path: findings.path().to_path_buf(),
                why,
            });
        }
    };
    let rows = JsonlLedger::for_project(project)?.read()?.errands;
    let now = deps.clock.now();
    let offset = deps.clock.offset();
    // Jev's questions are not counted against the caps (design §6.9)
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
    (deps.trace)(&format!(
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
        cancel,
        now,
        texts: HashMap::new(),
        mine: base.clone(),
        merged: HashSet::new(),
        new: Vec::new(),
        budget: Budget::new(caps, today),
        review_estimate: review_est,
        check_estimate: check_est,
        disk: &findings,
        written: base,
        renames: HashMap::new(),
    };
    let report = match run.go(scope, fresh) {
        Ok(r) => r,
        Err(Halt::Silent) => return Ok(Outcome::Skipped("Jev did not answer".into())),
        Err(Halt::Failed(e)) => return Err(e),
    };
    let (written, mine, mut renames) = (run.written, run.mine, run.renames);
    findings.update(|current| *current = merge_into(&written, &mine, current, &mut renames))?;
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
    drop(lock);
    Ok(Outcome::Ran(report))
}

/// A unit sent to the review call: its file, name, first line and the
/// fingerprint it was sent at.
type Sent = (PathBuf, String, u32, Fingerprint);

struct Run<'r, 'd> {
    cfg: &'r ScryConfig,
    st: &'r Steering,
    fences: &'r Fences,
    deps: &'r PassDeps<'d>,
    cancel: &'r Cancel,
    now: DateTime<Utc>,
    texts: HashMap<PathBuf, String>,
    /// The store as this pass leaves it.
    mine: ScryStore,
    /// Stored findings a finding of this pass merged into.
    merged: HashSet<FindingId>,
    new: Vec<FindingId>,
    budget: Budget,
    review_estimate: f64,
    check_estimate: f64,
    disk: &'r Store<ScryStore>,
    /// The store as this pass last wrote it (at first, as it read it), in
    /// written ids.
    written: ScryStore,
    /// The pass's ids that clashed with findings added meanwhile, to the
    /// ids written in their place.
    renames: HashMap<FindingId, FindingId>,
}

impl Run<'_, '_> {
    fn go(&mut self, scope: &Scope, fresh: bool) -> std::result::Result<Report, Halt> {
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

        // place, match, filter
        self.t(&section("Findings"));
        if raws.is_empty() {
            self.t("- none found\n");
        }
        for (n, raw) in raws.into_iter().enumerate() {
            self.t(&sub(&format!(
                "Finding {}: {} at {}:{}-{}",
                n + 1,
                raw.issue_type,
                raw.file.display(),
                raw.lines.start,
                raw.lines.end
            )));
            self.judge(raw, scope)?;
        }

        // resolved by the review call's answer
        let now = self.now;
        let mut remember = section("Remember");
        for f in &mut self.mine.findings {
            if no_longer.contains(&f.id.0) && matches!(f.status, Status::Open | Status::Waiting) {
                f.set_status(Status::Resolved, now);
                remember.push_str(&format!(
                    "- {} resolved: the reviewer said it no longer holds\n",
                    f.id.0
                ));
            }
        }
        for id in &gone {
            remember.push_str(&format!(
                "- {} gone: its unit or file is no longer there\n",
                id.0
            ));
        }

        // a unit whose text changed while the pass ran keeps no new findings
        // and is not reviewed
        let moved = self.drop_moved(&answered);
        for (file, name) in &moved {
            remember.push_str(&format!(
                "- `{}` `{}` changed while the pass ran: its new findings are dropped, it is not reviewed\n",
                file.display(),
                name
            ));
        }
        let kept: Vec<&Sent> = answered
            .iter()
            .filter(|(file, name, _, _)| !moved.contains(&(file.clone(), name.clone())))
            .collect();

        // not found again: an open or waiting finding whose unit this pass
        // looked at with a new fingerprint, that the review call neither
        // re-raised, said still holds, nor resolved, is resolved; only when
        // the pass was whole
        let whole = capped == 0 && failures.is_empty();
        if whole {
            let at: HashMap<(&Path, &str), Fingerprint> = kept
                .iter()
                .map(|(file, name, _, fp)| ((file.as_path(), name.as_str()), *fp))
                .collect();
            for f in &mut self.mine.findings {
                let sent_at = f
                    .place
                    .unit
                    .as_deref()
                    .and_then(|u| at.get(&(f.place.file.as_path(), u)));
                let not_found = base_ids.contains(&f.id)
                    && matches!(f.status, Status::Open | Status::Waiting)
                    && !self.merged.contains(&f.id)
                    && !no_longer.contains(&f.id.0)
                    && !held.contains(&f.id.0)
                    && matches!((sent_at, f.place.fingerprint), (Some(now_fp), Some(was)) if *now_fp != was);
                if not_found {
                    f.set_status(Status::Resolved, now);
                    remember.push_str(&format!(
                        "- {} resolved: its unit changed and the reviewer did not raise it again\n",
                        f.id.0
                    ));
                }
            }
        } else {
            remember.push_str(
                "- the pass was not whole (a cap or a failure): nothing is resolved for not being found again\n",
            );
        }
        if remember.trim_end().ends_with("## Remember") {
            remember.push_str("- nothing resolved or gone\n");
        }
        self.t(&remember);

        // the verification rounds, file by file
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
            let mut save = |s: &ScryStore| -> Result<()> {
                disk.update(|current| *current = merge_into(written, s, current, renames))?;
                *written = renamed(s, renames);
                Ok(())
            };
            let mut around = Around {
                fences,
                now: *now,
                old_waiting: &old_waiting,
                check_estimate: *check_estimate,
                save: &mut save,
            };
            let v = verify(mine, &sf.file, cfg, budget, deps, cancel, &mut around)?;
            checks_capped |= v.capped;
        }

        // checked again: a unit may have changed while the checks ran
        let moved = self.drop_moved(&answered);
        let mut written = section("Reviewed");
        for (file, name) in &moved {
            written.push_str(&format!(
                "- `{}` `{}` changed while the checks ran: not reviewed\n",
                file.display(),
                name
            ));
        }
        for (file, name, _, fp) in answered
            .iter()
            .filter(|(file, name, _, _)| !moved.contains(&(file.clone(), name.clone())))
        {
            written.push_str(&format!(
                "- `{}` `{name}` reviewed at {fp}\n",
                file.display()
            ));
            self.mine
                .reviewed
                .entry(file.clone())
                .or_default()
                .insert(fp.to_string());
            if let Some(names) = self.mine.capped.get_mut(file) {
                names.remove(name);
                if names.is_empty() {
                    self.mine.capped.remove(file);
                }
            }
        }

        if written.trim_end().ends_with("## Reviewed") {
            written.push_str("- no unit newly reviewed\n");
        }
        self.t(&written);

        Ok(Report {
            looked_at: answered.len(),
            new: self.new.clone(),
            spent: self.budget.spent(),
            capped,
            failures,
            whole,
            checks_capped,
            touched: Vec::new(),
        })
    }

    /// Drops this pass's new findings in units whose text changed since
    /// they were sent; returns those units.
    fn drop_moved(&mut self, answered: &[Sent]) -> HashSet<(PathBuf, String)> {
        let moved = self.moved(answered);
        let new_ids: HashSet<FindingId> = self.new.iter().cloned().collect();
        let dropped: HashSet<FindingId> = self
            .mine
            .findings
            .iter()
            .filter(|f| new_ids.contains(&f.id) && in_moved(&moved, &f.place))
            .map(|f| f.id.clone())
            .collect();
        self.mine.findings.retain(|f| !dropped.contains(&f.id));
        self.new.retain(|i| !dropped.contains(i));
        moved
    }

    /// Open, waiting and withheld findings of a file in scope whose unit is
    /// no longer in it (or whose file was deleted) are gone.
    fn sweep_gone(&mut self, scope: &Scope) -> Vec<FindingId> {
        let now = self.now;
        let mut gone = Vec::new();
        for sf in &scope.files {
            for f in &mut self.mine.findings {
                let left = sf.deleted
                    || f.place
                        .unit
                        .as_ref()
                        .is_some_and(|u| !sf.units_now.contains(u));
                if f.place.file == sf.file
                    && left
                    && matches!(f.status, Status::Open | Status::Waiting | Status::Withheld)
                {
                    f.set_status(Status::Gone, now);
                    gone.push(f.id.clone());
                }
            }
        }
        gone
    }

    fn t(&self, text: &str) {
        (self.deps.trace)(text);
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

    /// One batch to Jev; any answer it could not give stops the pass.
    fn ask(&self, qs: &[Question]) -> std::result::Result<Vec<Weighed>, Halt> {
        let ws = self
            .deps
            .decider
            .ask(qs, self.cancel)
            .map_err(|_| Halt::Silent)?;
        if ws.len() != qs.len() || ws.iter().any(|w| w.answer == Answer::Unsure) {
            return Err(Halt::Silent);
        }
        Ok(ws)
    }

    /// Jev's nitpick question on RAW: its confidence, and what it was asked on.
    fn confidence(
        &self,
        raw: &RawFinding,
        unit_fp: Option<&Fingerprint>,
        code: &str,
    ) -> std::result::Result<(f64, String, Weighed), Halt> {
        let q = nitpick(self.st, raw, code);
        let on = asked_on(unit_fp, rule_context(&q), self.st);
        let w = self.ask(std::slice::from_ref(&q))?;
        let w = w.into_iter().next().expect("one answer for one question");
        Ok((confidence_of(&w), on, w))
    }

    fn judge(&mut self, mut raw: RawFinding, scope: &Scope) -> std::result::Result<(), Halt> {
        let cited_rule = raw.rule.clone();
        raw.rule = raw
            .rule
            .as_deref()
            .and_then(|r| self.st.resolve_rule(r))
            .map(String::from);
        let evidence: Vec<String> = raw
            .evidence
            .iter()
            .map(|(f, l)| format!("{}:{}-{}", f.display(), l.start, l.end))
            .collect();
        let rule = match (&cited_rule, &raw.rule) {
            (None, _) => "none cited".to_string(),
            (Some(c), Some(r)) => format!("cited \"{c}\", resolved to \"{r}\""),
            (Some(c), None) => {
                format!("cited \"{c}\", no rule matches: the rule is dropped, the finding stays")
            }
        };
        self.t(&format!(
            "- statement: {}\n- rule: {rule}\n- evidence: {}\n",
            raw.statement,
            if evidence.is_empty() {
                "none".into()
            } else {
                evidence.join(", ")
            }
        ));
        let units_now = if self.fences.allows(&raw.file) {
            self.deps.view.units(&raw.file).unwrap_or_default()
        } else {
            Vec::new()
        };
        let (pl, status) = place(&raw, &scope.units, &units_now);
        let unit = pl
            .unit
            .as_deref()
            .map_or("no unit".into(), |u| format!("unit `{u}`"));
        if status == Status::Elsewhere {
            let id = self.add(&raw, pl, Status::Elsewhere, Scores::default());
            self.t(&format!(
                "- placed: elsewhere (its lines are in no unit in scope; {unit} now): stored with no question\n- id {}, elsewhere\n",
                id.0
            ));
            return Ok(());
        }
        self.t(&format!(
            "- placed: {unit} (fingerprint {})\n",
            pl.fingerprint.map_or("none".into(), |f| f.to_string())
        ));
        let code_a = cited_code(&self.text(&raw.file), raw.lines.start);
        let cands: Vec<Finding> = candidates(&self.mine, &raw).into_iter().cloned().collect();
        let mut hit = None;
        if !cands.is_empty() {
            let a = finding_text(
                &raw.issue_type,
                &raw.file,
                raw.lines.start,
                raw.rule.as_deref(),
                &raw.statement,
            );
            let mut qs = Vec::new();
            for c in &cands {
                let b = finding_text(
                    &c.issue_type,
                    &c.place.file,
                    c.place.lines.start,
                    c.rule.as_deref(),
                    &c.statement,
                );
                let code_b = cited_code(&self.text(&c.place.file), c.place.lines.start);
                qs.push(same_problem(&a, &b, &code_a, &code_b));
            }
            let ws = self.ask(&qs)?;
            let mut lines = String::new();
            let mut first = true;
            for (c, w) in cands.iter().zip(&ws) {
                let p = p_yes(w);
                let said = if p >= SAME_THRESHOLD && first {
                    first = false;
                    "the same: the first at or past the threshold wins"
                } else if p >= SAME_THRESHOLD {
                    "the same, but an earlier one won"
                } else {
                    "not the same"
                };
                lines.push_str(&format!(
                    "- same problem as {} ({}, {})? {} → {said}\n",
                    c.id.0,
                    status_word(c.status),
                    finding_text(
                        &c.issue_type,
                        &c.place.file,
                        c.place.lines.start,
                        c.rule.as_deref(),
                        &c.statement
                    ),
                    p_text(w),
                ));
            }
            self.t(&lines);
            hit = cands
                .into_iter()
                .zip(ws)
                .find(|(_, w)| p_yes(w) >= SAME_THRESHOLD);
        } else {
            self.t(
                "- same-problem question: no stored finding shares its file, rule or cited code\n",
            );
        }
        match hit.map(|(c, _)| (match_stored(&c, pl.unit.as_deref()), c)) {
            Some((Match::Drop, c)) => {
                self.t(&format!(
                    "- result: dropped ({} was {} here)\n",
                    c.id.0,
                    status_word(c.status)
                ));
                Ok(())
            }
            Some((Match::Merge, c)) => {
                self.t(&format!("- result: merge into {}\n", c.id.0));
                self.merge(&c.id, raw, pl)
            }
            other => {
                if let Some((_, c)) = other {
                    self.t(&format!(
                        "- result: new ({} is {}: a match starts afresh)\n",
                        c.id.0,
                        status_word(c.status)
                    ));
                } else {
                    self.t("- result: new\n");
                }
                let (confidence, on, w) =
                    self.confidence(&raw, pl.fingerprint.as_ref(), &code_a)?;
                let status = if confidence >= self.cfg.confidence_threshold {
                    Status::Waiting
                } else {
                    Status::Withheld
                };
                let scores = Scores {
                    confidence: Some(confidence),
                    asked_on: Some(on),
                };
                let id = self.add(&raw, pl, status, scores);
                self.t(&format!(
                    "- nitpick? {} → confidence {confidence:.2}, threshold {:.2} → {}\n- id {}, {}\n",
                    p_text(&w),
                    self.cfg.confidence_threshold,
                    status_word(status),
                    id.0,
                    status_word(status)
                ));
                Ok(())
            }
        }
    }

    fn add(&mut self, raw: &RawFinding, pl: Place, status: Status, scores: Scores) -> FindingId {
        let id = self
            .mine
            .new_id(self.now, &pl.file, pl.lines, &raw.statement);
        self.new.push(id.clone());
        self.mine.findings.push(Finding {
            id,
            issue_type: raw.issue_type.clone(),
            rule: raw.rule.clone(),
            statement: raw.statement.clone(),
            place: pl,
            evidence: bare(&raw.evidence),
            scores,
            verdict: None,
            status,
            seen: 1,
            first: self.now,
            last: self.now,
            history: vec![(self.now, status)],
            model: self.cfg.review_model.clone(),
            cost_usd: 0.0,
        });
        self.new.last().cloned().expect("just pushed")
    }

    /// RAW merges into the stored finding ID: its place, wording and evidence
    /// replace the stored ones; id, status and dismissals stay. A withheld one
    /// is asked again when what it was asked on changed; a refuted or unsure
    /// one waits again when its evidence prints moved.
    fn merge(
        &mut self,
        id: &FindingId,
        raw: RawFinding,
        pl: Place,
    ) -> std::result::Result<(), Halt> {
        let cited = citations(self.deps.view, self.fences, &raw.evidence);
        let old = self.mine.findings.iter().find(|f| f.id == *id).cloned();
        let Some(mut f) = old else { return Ok(()) };
        let old_prints: Vec<Option<String>> = f.evidence.iter().map(|e| e.print.clone()).collect();
        // a finding stored by an earlier pass takes this pass's place, wording
        // and evidence; one this pass found keeps the first of its duplicates'
        if !self.new.contains(&f.id) {
            f.place = pl;
            f.statement = raw.statement;
            f.evidence = cited.clone().unwrap_or_else(|_| bare(&raw.evidence));
        }
        f.seen += 1;
        f.last = self.now;
        match f.status {
            Status::Withheld => {
                let r = raw_of(&f);
                let code = cited_code(&self.text(&f.place.file), f.place.lines.start);
                let q = nitpick(self.st, &r, &code);
                let on = asked_on(f.place.fingerprint.as_ref(), rule_context(&q), self.st);
                if f.scores.asked_on.as_deref() != Some(on.as_str()) {
                    let w = self.ask(std::slice::from_ref(&q))?;
                    let confidence = confidence_of(&w[0]);
                    f.scores = Scores {
                        confidence: Some(confidence),
                        asked_on: Some(on),
                    };
                    if confidence >= self.cfg.confidence_threshold {
                        f.set_status(Status::Waiting, self.now);
                    }
                    self.t(&format!(
                        "- withheld, and what it was asked on changed: nitpick? {} → confidence {confidence:.2}, threshold {:.2} → {}\n",
                        p_text(&w[0]),
                        self.cfg.confidence_threshold,
                        status_word(f.status)
                    ));
                } else {
                    self.t("- withheld, asked on the same unit, rule and sides: not asked again\n");
                }
            }
            Status::Refuted | Status::Unsure => {
                if let Ok(new) = &cited {
                    let new_prints: Vec<Option<String>> =
                        new.iter().map(|e| e.print.clone()).collect();
                    if new_prints != old_prints {
                        f.set_status(Status::Waiting, self.now);
                        self.t("- its evidence moved: it waits to be checked again\n");
                    }
                }
            }
            _ => {}
        }
        self.t(&format!(
            "- id {} (merged; seen {}), {}\n",
            f.id.0,
            count(f.seen as usize, "time"),
            status_word(f.status)
        ));
        self.merged.insert(f.id.clone());
        if let Some(slot) = self.mine.findings.iter_mut().find(|g| g.id == f.id) {
            *slot = f;
        }
        Ok(())
    }

    /// The sent units whose fingerprint is not what was sent (or that are
    /// no longer in their file).
    fn moved(&self, sent: &[Sent]) -> HashSet<(PathBuf, String)> {
        let mut now: BTreeMap<&Path, Vec<Unit>> = BTreeMap::new();
        for (file, _, _, _) in sent {
            now.entry(file.as_path())
                .or_insert_with(|| self.deps.view.units(file).unwrap_or_default());
        }
        sent.iter()
            .filter(|(file, name, _, fp)| {
                !now[file.as_path()]
                    .iter()
                    .any(|u| u.name == *name && u.fingerprint == *fp)
            })
            .map(|(file, name, _, _)| (file.clone(), name.clone()))
            .collect()
    }
}

/// `P(yes) 0.20`, or the answer when it carries no probability.
fn p_text(w: &Weighed) -> String {
    match w.answer.p_yes() {
        Some(p) => format!("P(yes) {p:.2}"),
        None => format!("{:?}", w.answer),
    }
}

/// A status as the store writes it (`open`, `dismissed-here`, …).
pub fn status_word(s: Status) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn p_yes(w: &Weighed) -> f64 {
    match w.answer {
        Answer::Yes(Confidence::Known(c)) => c,
        Answer::No(Confidence::Known(c)) => 1.0 - c,
        _ => 0.0,
    }
}

/// The rule text a nitpick question carried: the rule, or the Rules section.
fn rule_context(q: &Question) -> &str {
    q.context
        .iter()
        .find(|(n, _)| n == "project rule" || n == "project rules")
        .map_or("", |(_, v)| v.as_str())
}

fn raw_of(f: &Finding) -> RawFinding {
    RawFinding {
        issue_type: f.issue_type.clone(),
        file: f.place.file.clone(),
        lines: f.place.lines,
        statement: f.statement.clone(),
        rule: f.rule.clone(),
        evidence: f
            .evidence
            .iter()
            .map(|e| (e.file.clone(), e.lines))
            .collect(),
    }
}

fn bare(evidence: &[(PathBuf, sorcerer_core::domain::unit::Span)]) -> Vec<Evidence> {
    evidence
        .iter()
        .map(|(file, lines)| Evidence {
            file: file.clone(),
            lines: *lines,
            print: None,
        })
        .collect()
}

fn in_moved(moved: &HashSet<(PathBuf, String)>, pl: &Place) -> bool {
    pl.unit
        .as_ref()
        .is_some_and(|u| moved.contains(&(pl.file.clone(), u.clone())))
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

/// The pass's store (MINE, made from BASE) laid over CURRENT, the store as
/// it is now (olifant's `mergeInto`): a finding the pass changed takes the
/// pass's version only when its status in CURRENT is still what the pass
/// started from, so a dismissal made meanwhile stands; the pass's new
/// findings are added; reviewed sets are joined. A new finding whose id
/// was taken meanwhile (by a finding added from elsewhere) takes a fresh
/// one; RENAMES maps the pass's ids to the ids written, is applied to MINE
/// first and grows with each such clash. BASE is in written ids.
pub fn merge_into(
    base: &ScryStore,
    mine: &ScryStore,
    current: &ScryStore,
    renames: &mut HashMap<FindingId, FindingId>,
) -> ScryStore {
    let mine = renamed(mine, renames);
    let by_id = |s: &ScryStore| -> HashMap<FindingId, Finding> {
        s.findings
            .iter()
            .map(|f| (f.id.clone(), f.clone()))
            .collect()
    };
    let (b, m) = (by_id(base), by_id(&mine));
    let mut taken: HashSet<String> = current
        .findings
        .iter()
        .chain(&mine.findings)
        .map(|f| f.id.0.clone())
        .collect();
    let cur: HashSet<&FindingId> = current.findings.iter().map(|f| &f.id).collect();
    let mut findings: Vec<Finding> = current
        .findings
        .iter()
        .filter(|f| !(b.contains_key(&f.id) && !m.contains_key(&f.id)))
        .map(|f| match (b.get(&f.id), m.get(&f.id)) {
            (Some(was), Some(ours)) if was.status == f.status => ours.clone(),
            _ => f.clone(),
        })
        .collect();
    for f in mine.findings.iter().filter(|f| !b.contains_key(&f.id)) {
        let mut f = f.clone();
        if cur.contains(&f.id) {
            let id = free_id(&f.id, &taken);
            taken.insert(id.0.clone());
            renames.insert(f.id.clone(), id.clone());
            f.id = id;
        }
        findings.push(f);
    }
    let mut reviewed = current.reviewed.clone();
    for (file, set) in &mine.reviewed {
        reviewed
            .entry(file.clone())
            .or_default()
            .extend(set.iter().cloned());
    }
    let capped = if mine.capped == base.capped {
        current.capped.clone()
    } else {
        mine.capped.clone()
    };
    ScryStore {
        findings,
        reviewed,
        capped,
    }
}

/// STORE with its finding ids mapped through RENAMES.
pub fn renamed(store: &ScryStore, renames: &HashMap<FindingId, FindingId>) -> ScryStore {
    let mut s = store.clone();
    for f in &mut s.findings {
        if let Some(to) = renames.get(&f.id) {
            f.id = to.clone();
        }
    }
    s
}

/// Six hex digits from ID and a counter, not in TAKEN.
fn free_id(id: &FindingId, taken: &HashSet<String>) -> FindingId {
    (1..)
        .map(|n: u64| fnv64_hex(format!("{}\0{n}", id.0).as_bytes())[..6].to_string())
        .find(|c| !taken.contains(c))
        .map(FindingId)
        .expect("an unbounded counter finds a free id")
}
