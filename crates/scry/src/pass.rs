//! The pass (design §6.4, olifant's `Inspect/Pass.hs`): the scope's units
//! not yet inspected go to the find call in packs; each finding is placed,
//! matched against the store, filtered by Jev's nitpick question and
//! remembered; then each file's waiting findings go through the
//! verification rounds (`rounds`), within the caps.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, Local, Utc};
use sorcerer_core::cost::spent_in;
use sorcerer_core::fences::Fences;
use sorcerer_core::fingerprint::Fingerprint;
use sorcerer_core::fnv::fnv64_hex;
use sorcerer_core::ledger::Ledger;
use sorcerer_core::store::{Snapshot, Store};
use sorcerer_core::unit::Unit;
use sorcerer_core::view::CodebaseView;
use sorcerer_core::{Cancel, Error, Project, Result};
use sorcerer_models::checker::{Checked, Claim};
use sorcerer_models::decide::{Answer, Confidence, Decider, Question, Weighed};

use crate::config::{ScryConfig, scry_config};
use crate::find::{FindAsk, FindUnit, Finder, RawFinding, pack_units, parse_find};
use crate::finding::{Evidence, Finding, Id, Place, Scores, Status};
use crate::judge::{
    Match, SAME_CUT, asked_on, candidates, citations, cited_code, finding_text, keep_of,
    match_stored, nitpick, place, same_problem,
};
use crate::rounds::{Around, Budget, check_estimate, find_estimate, verify};
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
    /// A find call answered with this many findings.
    Found(usize),
    /// The store was written.
    Stored,
}

pub struct PassDeps<'a> {
    pub view: &'a dyn CodebaseView,
    pub finder: &'a dyn Finder,
    pub decider: &'a dyn Decider,
    pub checker: Checker<'a>,
    pub clock: &'a dyn Clock,
    pub progress: &'a (dyn Fn(&Progress) + Sync),
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
    /// Units sent to a find call that answered.
    pub looked_at: usize,
    /// Findings this pass added to the store.
    pub new: Vec<Id>,
    pub spent: f64,
    /// Units a cap held back.
    pub capped: usize,
    /// One line per find call that failed.
    pub failures: Vec<String>,
    /// Nothing capped and every find call answered.
    pub whole: bool,
    /// A claim check did not fit the caps; its finding waits.
    pub checks_capped: bool,
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

/// One pass over SCOPE. With FRESH, units already inspected are sent again.
/// Holds `state/scry/pass.lock` throughout; writes the store before a
/// round's claim checks start and at the end, each time laying its changes
/// over the store as it is then. CAPS hold the find calls and claim checks
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
    let rows = Ledger::for_project(project)?.read()?.errands;
    let now = deps.clock.now();
    let offset = deps.clock.offset();
    // Jev's questions are not counted against the caps (design §6.9)
    let paid: Vec<_> = rows
        .iter()
        .filter(|e| e.verb == "find" || e.verb == "check")
        .cloned()
        .collect();
    let today = spent_in(
        &paid,
        "scry",
        now.with_timezone(&offset).date_naive(),
        offset,
    );
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
        find_estimate: find_estimate(&rows, &cfg.find_model),
        check_estimate: check_estimate(&rows),
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
    for id in &mut report.new {
        if let Some(to) = renames.get(id) {
            *id = to.clone();
        }
    }
    (deps.progress)(&Progress::Stored);
    drop(lock);
    Ok(Outcome::Ran(report))
}

/// A unit sent to the find call: its file, name, first line and the
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
    merged: HashSet<Id>,
    new: Vec<Id>,
    budget: Budget,
    find_estimate: f64,
    check_estimate: f64,
    disk: &'r Store<ScryStore>,
    /// The store as this pass last wrote it (at first, as it read it), in
    /// written ids.
    written: ScryStore,
    /// The pass's ids that clashed with findings added meanwhile, to the
    /// ids written in their place.
    renames: HashMap<Id, Id>,
}

impl Run<'_, '_> {
    fn go(&mut self, scope: &Scope, fresh: bool) -> std::result::Result<Report, Halt> {
        let base_ids: HashSet<Id> = self.mine.findings.iter().map(|f| f.id.clone()).collect();
        let old_waiting: HashSet<Id> = self
            .mine
            .findings
            .iter()
            .filter(|f| f.status == Status::Waiting)
            .map(|f| f.id.clone())
            .collect();
        self.sweep_gone(scope);

        // the units to send, packed
        let to_send: Vec<&ScopedUnit> = scope
            .units
            .iter()
            .filter(|u| {
                fresh
                    || !self
                        .mine
                        .inspected
                        .get(&u.file)
                        .is_some_and(|s| s.contains(u.unit.fingerprint.as_str()))
            })
            .collect();
        let mut prints: HashMap<(PathBuf, String, u32), Fingerprint> = HashMap::new();
        let mut units = Vec::new();
        for u in &to_send {
            prints.insert(
                (u.file.clone(), u.unit.name.clone(), u.unit.span.start),
                u.unit.fingerprint,
            );
            units.push(FindUnit {
                file: u.file.clone(),
                name: u.unit.name.clone(),
                lines: u.unit.span,
                header: u.header.clone(),
                text: lines_of(&self.text(&u.file), &u.unit),
                in_progress: false,
            });
        }

        // the find calls, one a pack, while the caps allow; once one pack
        // is capped every later one is, and its units are recorded for later
        let mut capped = 0;
        let mut failures = Vec::new();
        let mut answered: Vec<Sent> = Vec::new();
        let mut raws: Vec<RawFinding> = Vec::new();
        let mut no_longer: HashSet<String> = HashSet::new();
        let mut held: HashSet<String> = HashSet::new();
        for pack in pack_units(units) {
            let files = files_of(&pack);
            if capped > 0 || !self.budget.allows(self.find_estimate) {
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
                failures.push(format!("find cancelled for {}", names(&files)));
                continue;
            }
            let ask = FindAsk {
                diff: diff_of(scope, &files),
                steering: self.st.text.clone(),
                reported: Vec::new(),
                open: self.open_on(&pack),
                units: pack,
            };
            // one call's cap: what the caps leave, never more than a pass
            // may spend (on the command line nothing else bounds it, and a
            // timeout is charged its cap)
            let cap = self
                .budget
                .room()
                .map_or(self.cfg.pass_cost, |r| r.min(self.cfg.pass_cost));
            let found = self
                .deps
                .finder
                .find(&ask, &self.cfg.find_model, cap, self.cancel);
            // an unpriced answer is charged its estimate, a timeout the cap
            // it ran under (what its errand records), any other failure nothing
            self.budget.charge(match &found {
                Ok(f) => f.errand.cost_usd.unwrap_or(self.find_estimate),
                Err(sorcerer_models::Error::Timeout { .. }) => cap,
                Err(_) => 0.0,
            });
            let reply = match found {
                Err(e) => Err(e.to_string()),
                Ok(f) => parse_find(&f.reply),
            };
            match reply {
                Err(why) => failures.push(format!("find failed for {}: {why}", names(&files))),
                Ok(r) => {
                    (self.deps.progress)(&Progress::Found(r.findings.len()));
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
        for raw in raws {
            self.judge(raw, scope)?;
        }

        // resolved by the find call's answer
        let now = self.now;
        for f in &mut self.mine.findings {
            if no_longer.contains(&f.id.0) && matches!(f.status, Status::Open | Status::Waiting) {
                f.set_status(Status::Resolved, now);
            }
        }

        // a unit whose text changed while the pass ran keeps no new findings
        // and is not inspected
        let moved = self.drop_moved(&answered);
        let kept: Vec<&Sent> = answered
            .iter()
            .filter(|(file, name, _, _)| !moved.contains(&(file.clone(), name.clone())))
            .collect();

        // not found again: an open or waiting finding whose unit this pass
        // looked at with a new fingerprint, that the find call neither
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
                }
            }
        }

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
        for (file, name, _, fp) in answered
            .iter()
            .filter(|(file, name, _, _)| !moved.contains(&(file.clone(), name.clone())))
        {
            self.mine
                .inspected
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

        Ok(Report {
            looked_at: answered.len(),
            new: self.new.clone(),
            spent: self.budget.spent(),
            capped,
            failures,
            whole,
            checks_capped,
        })
    }

    /// Drops this pass's new findings in units whose text changed since
    /// they were sent; returns those units.
    fn drop_moved(&mut self, answered: &[Sent]) -> HashSet<(PathBuf, String)> {
        let moved = self.moved(answered);
        let new_ids: HashSet<Id> = self.new.iter().cloned().collect();
        let dropped: HashSet<Id> = self
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
    fn sweep_gone(&mut self, scope: &Scope) {
        let now = self.now;
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
                }
            }
        }
    }

    /// The open and waiting findings on PACK's units, for the find call to answer.
    fn open_on(&self, pack: &[FindUnit]) -> Vec<Finding> {
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

    /// Jev's nitpick question on RAW: keep, and what it was asked on.
    fn keep(
        &self,
        raw: &RawFinding,
        unit_fp: Option<&Fingerprint>,
        code: &str,
    ) -> std::result::Result<(f64, String), Halt> {
        let q = nitpick(self.st, raw, code);
        let on = asked_on(unit_fp, rule_context(&q), self.st);
        let w = self.ask(std::slice::from_ref(&q))?;
        Ok((keep_of(&w[0]), on))
    }

    fn judge(&mut self, mut raw: RawFinding, scope: &Scope) -> std::result::Result<(), Halt> {
        raw.rule = raw
            .rule
            .as_deref()
            .and_then(|r| self.st.resolve_rule(r))
            .map(String::from);
        let units_now = if self.fences.allows(&raw.file) {
            self.deps.view.units(&raw.file).unwrap_or_default()
        } else {
            Vec::new()
        };
        let (pl, status) = place(&raw, &scope.units, &units_now);
        if status == Status::Elsewhere {
            self.add(&raw, pl, Status::Elsewhere, Scores::default());
            return Ok(());
        }
        let code_a = cited_code(&self.text(&raw.file), raw.lines.start);
        let cands: Vec<Finding> = candidates(&self.mine, &raw).into_iter().cloned().collect();
        let mut hit = None;
        if !cands.is_empty() {
            let a = finding_text(
                &raw.kind,
                &raw.file,
                raw.lines.start,
                raw.rule.as_deref(),
                &raw.statement,
            );
            let mut qs = Vec::new();
            for c in &cands {
                let b = finding_text(
                    &c.kind,
                    &c.place.file,
                    c.place.lines.start,
                    c.rule.as_deref(),
                    &c.statement,
                );
                let code_b = cited_code(&self.text(&c.place.file), c.place.lines.start);
                qs.push(same_problem(&a, &b, &code_a, &code_b));
            }
            let ws = self.ask(&qs)?;
            hit = cands
                .into_iter()
                .zip(ws)
                .find(|(_, w)| p_yes(w) >= SAME_CUT);
        }
        match hit.map(|(c, _)| (match_stored(&c, pl.unit.as_deref()), c)) {
            Some((Match::Drop, _)) => Ok(()),
            Some((Match::Merge, c)) => self.merge(&c.id, raw, pl),
            _ => {
                let (keep, on) = self.keep(&raw, pl.fingerprint.as_ref(), &code_a)?;
                let status = if keep >= self.cfg.cut {
                    Status::Waiting
                } else {
                    Status::Withheld
                };
                let scores = Scores {
                    keep: Some(keep),
                    asked_on: Some(on),
                };
                self.add(&raw, pl, status, scores);
                Ok(())
            }
        }
    }

    fn add(&mut self, raw: &RawFinding, pl: Place, status: Status, scores: Scores) {
        let id = self
            .mine
            .new_id(self.now, &pl.file, pl.lines, &raw.statement);
        self.new.push(id.clone());
        self.mine.findings.push(Finding {
            id,
            kind: raw.kind.clone(),
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
            model: self.cfg.find_model.clone(),
            cost_usd: 0.0,
        });
    }

    /// RAW merges into the stored finding ID: its place, wording and evidence
    /// replace the stored ones; id, status and dismissals stay. A withheld one
    /// is asked again when what it was asked on changed; a refuted or unsure
    /// one waits again when its evidence prints moved.
    fn merge(&mut self, id: &Id, raw: RawFinding, pl: Place) -> std::result::Result<(), Halt> {
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
                    let keep = keep_of(&w[0]);
                    f.scores = Scores {
                        keep: Some(keep),
                        asked_on: Some(on),
                    };
                    if keep >= self.cfg.cut {
                        f.set_status(Status::Waiting, self.now);
                    }
                }
            }
            Status::Refuted | Status::Unsure => {
                if let Ok(new) = &cited {
                    let new_prints: Vec<Option<String>> =
                        new.iter().map(|e| e.print.clone()).collect();
                    if new_prints != old_prints {
                        f.set_status(Status::Waiting, self.now);
                    }
                }
            }
            _ => {}
        }
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
        kind: f.kind.clone(),
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

fn bare(evidence: &[(PathBuf, sorcerer_core::unit::Span)]) -> Vec<Evidence> {
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
fn files_of(pack: &[FindUnit]) -> Vec<PathBuf> {
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
/// findings are added; inspected sets are joined. A new finding whose id
/// was taken meanwhile (by a finding added from elsewhere) takes a fresh
/// one; RENAMES maps the pass's ids to the ids written, is applied to MINE
/// first and grows with each such clash. BASE is in written ids.
pub fn merge_into(
    base: &ScryStore,
    mine: &ScryStore,
    current: &ScryStore,
    renames: &mut HashMap<Id, Id>,
) -> ScryStore {
    let mine = renamed(mine, renames);
    let by_id = |s: &ScryStore| -> HashMap<Id, Finding> {
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
    let cur: HashSet<&Id> = current.findings.iter().map(|f| &f.id).collect();
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
    let mut inspected = current.inspected.clone();
    for (file, set) in &mine.inspected {
        inspected
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
        inspected,
        capped,
    }
}

/// STORE with its finding ids mapped through RENAMES.
pub fn renamed(store: &ScryStore, renames: &HashMap<Id, Id>) -> ScryStore {
    let mut s = store.clone();
    for f in &mut s.findings {
        if let Some(to) = renames.get(&f.id) {
            f.id = to.clone();
        }
    }
    s
}

/// Six hex digits from ID and a counter, not in TAKEN.
fn free_id(id: &Id, taken: &HashSet<String>) -> Id {
    (1..)
        .map(|n: u64| fnv64_hex(format!("{}\0{n}", id.0).as_bytes())[..6].to_string())
        .find(|c| !taken.contains(c))
        .map(Id)
        .expect("an unbounded counter finds a free id")
}
