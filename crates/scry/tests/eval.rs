//! The eval: 58 bistro cases (`corpus/scry/`) run through whole passes with
//! every model call replayed, scored against its passing scores. The find calls replay
//! 1a's sonnet findings (split by the pack's files), the claim checks 1a's
//! verdicts, and Jev its recording under `corpus/scry/jev/`. The passing scores are
//! inspect's (V-must >= 16/18 with a hit shown, B >= 6/8, at most 0.5 shown a
//! T or N case on average).
//!
//! `SCRY_EVAL_CASES=b01,vm02` runs only those cases and prints their
//! findings (the passing scores are then not asserted: a look, not a measure).
//! `SCRY_EVAL_LOG=DIR` writes each case's scope and pass trace (the run
//! log's middle, as `scry -v` would stream it) to `DIR/<id>.md`.
//! `SCRY_RECORD_JEV=1` (with `JEV_API_KEY`) asks Jev what is not recorded:
//! an outward call, run by hand, never in CI.
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use scry::config::scry_config;
use scry::find::{FindAsk, Finder, Found};
use scry::pass::{Caps, Outcome, PassDeps, Progress, pass};
use scry::scope::{ScopeWhy, scope_of};
use scry::store::{ScryStore, store};
use serde_json::{Value, json};
use sorcerer_core::Cancel;
use sorcerer_core::env::SystemEnv;
use sorcerer_core::store::Snapshot;
use sorcerer_models::checker::{Checked, Claim, Verdict};
use sorcerer_models::decide::jev::question_key;
use sorcerer_models::decide::recorded::RecordedDecider;
use sorcerer_models::decide::{Decider, Question, Weighed};
use support::{FixedClock, errand, git};

const SETS: [&str; 5] = ["V-must", "B", "T", "N", "V-taste"];

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/scry")
}

fn read_json(p: &Path) -> Value {
    let text = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Case {
    id: String,
    set: String,
    hits: Vec<String>,
    changed: Vec<PathBuf>,
}

fn cases() -> Vec<Case> {
    let labels = read_json(&corpus().join("labels.json"));
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect()
    };
    labels
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, c)| Case {
            id: id.clone(),
            set: c["set"].as_str().unwrap().to_string(),
            hits: strings(&c["hits"]),
            changed: strings(&c["changed"])
                .into_iter()
                .map(PathBuf::from)
                .collect(),
        })
        .collect()
}

/// The find call replayed: the case's recorded findings whose file is one of
/// the pack's files, as one reply (an empty one when none are).
struct CorpusFinder {
    findings: Vec<Value>,
}

impl CorpusFinder {
    fn for_case(id: &str) -> CorpusFinder {
        let rec = read_json(&corpus().join("finds").join(format!("{id}.json")));
        let reply: Value = serde_json::from_str(rec["reply"].as_str().unwrap()).unwrap();
        CorpusFinder {
            findings: reply["findings"].as_array().cloned().unwrap_or_default(),
        }
    }
}

impl Finder for CorpusFinder {
    fn find(
        &self,
        ask: &FindAsk,
        model: &str,
        _cap_usd: f64,
        _cancel: &Cancel,
    ) -> sorcerer_models::Result<Found> {
        let files: BTreeSet<&Path> = ask.units.iter().map(|u| u.file.as_path()).collect();
        let mine: Vec<&Value> = self
            .findings
            .iter()
            .filter(|f| {
                f["file"]
                    .as_str()
                    .is_some_and(|p| files.contains(Path::new(p)))
            })
            .collect();
        Ok(Found {
            reply: json!({"findings": mine, "open": []}).to_string(),
            errand: errand("find", model, 0.04),
        })
    }
}

/// In a look (`SCRY_EVAL_CASES`), prints each question Jev is asked: its
/// kind, its recording's key, P(yes) and the finding it is about.
struct Looking<'a> {
    inner: &'a dyn Decider,
    case: &'a str,
}

impl Decider for Looking<'_> {
    fn ask(&self, qs: &[Question], cancel: &Cancel) -> sorcerer_models::Result<Vec<Weighed>> {
        let out = self.inner.ask(qs, cancel)?;
        for (q, w) in qs.iter().zip(&out) {
            let about = |name: &str| -> String {
                q.context
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.chars().take(70).collect())
                    .unwrap_or_default()
            };
            println!(
                "{} ask {} {} p_yes={:?} [{}] [{}]",
                self.case,
                q.kind,
                question_key(q),
                w.answer.p_yes(),
                about(if q.kind.starts_with("same") {
                    "finding A"
                } else {
                    "finding"
                }),
                about("finding B"),
            );
        }
        Ok(out)
    }
}

/// Copies the plain files under FROM to TO.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

struct Row {
    id: String,
    set: String,
    shown: usize,
    hit: bool,
}

fn run_case(c: &Case, checks: &BTreeMap<String, String>, record: bool, look: bool) -> Row {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    copy_tree(&corpus().join("base"), root);
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    let patch = corpus().join("cases").join(format!("{}.patch", c.id));
    git(root, &["apply", patch.to_str().unwrap()]);
    support::write(
        root,
        ".sorcerer/scry.md",
        &std::fs::read_to_string(corpus().join("steering.md")).unwrap(),
    );
    support::write(
        root,
        ".sorcerer/config.toml",
        "[languages]\nnames = [\"python\"]\n\n[scry]\nbudget_per_pass = 5.0\nbudget_per_day = 1000.0\n",
    );

    let (project, view) = support::project_with_vendored_grammars(root);
    let cfg = scry_config(&project).unwrap();
    let scope = scope_of(&project, &view, ScopeWhy::Working, false, &cfg).unwrap();
    let finder = CorpusFinder::for_case(&c.id);
    let mut decider = RecordedDecider::new(corpus().join("jev"));
    if record {
        // the live Jev, its ledger the temp project's
        decider = decider.recording_with(scry::live::jev_for(&project, &SystemEnv).unwrap());
    }
    let checker = |claim: &Claim| -> sorcerer_models::Result<Checked> {
        let verdict = match checks.get(&collapse(&claim.statement)).map(String::as_str) {
            Some("right") => Verdict::Right,
            Some("wrong") => Verdict::Wrong,
            _ => Verdict::CannotTell,
        };
        Ok(Checked {
            verdict,
            why: "1a's claim check".into(),
            citations: Vec::new(),
            cited: Vec::new(),
            escalated: false,
            errands: Vec::new(),
        })
    };
    let looking = Looking {
        inner: &decider,
        case: &c.id,
    };
    let decider: &dyn Decider = if look { &looking } else { &decider };
    let clock = FixedClock::at("2026-11-18T23:00:00Z".parse().unwrap());
    let progress = |_: &Progress| {};
    // SCRY_EVAL_LOG=DIR writes each case's scope and pass trace to DIR/<id>.md
    let log_dir = std::env::var_os("SCRY_EVAL_LOG").map(PathBuf::from);
    let log = std::cell::RefCell::new(format!(
        "# scry eval case {}\n{}",
        c.id,
        scry::log::scope_text(&scope)
    ));
    let collect = |s: &str| log.borrow_mut().push_str(s);
    let deps = PassDeps {
        view: &view,
        finder: &finder,
        decider,
        checker: &checker,
        clock: &clock,
        progress: &progress,
        trace: if log_dir.is_some() {
            &collect
        } else {
            &scry::log::quiet
        },
    };
    let caps = Caps {
        pass: Some(cfg.budget_per_pass),
        day: cfg.budget_per_day,
        budget: None,
    };
    match pass(&project, &scope, &caps, false, &deps, &Cancel::default()) {
        Ok(Outcome::Ran(r)) => assert!(
            r.failures.is_empty(),
            "case {}: failures {:?}",
            c.id,
            r.failures
        ),
        Ok(Outcome::Skipped(why)) => panic!("case {} skipped: {why}", c.id),
        Err(e) => panic!("case {}: {e}", c.id),
    }

    if let Some(d) = &log_dir {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join(format!("{}.md", c.id)), log.borrow().as_str()).unwrap();
    }

    let s: ScryStore = match store(&project).unwrap().read().unwrap() {
        Snapshot::Current(s) => s,
        Snapshot::Absent => ScryStore::default(),
        Snapshot::Unreadable(why) => panic!("case {}: {why}", c.id),
    };
    if look {
        for f in &s.findings {
            let first: String = f.statement.chars().take(90).collect();
            println!(
                "{} {} {:?} {} keep={:?} verdict={:?} unit={:?} lines={:?} {}",
                c.id,
                f.place.file.display(),
                f.status,
                f.issue_type,
                f.scores.confidence,
                f.verdict.as_ref().map(|v| &v.verdict),
                f.place.unit,
                f.place.lines,
                first
            );
        }
    }
    let mut shown: Vec<(PathBuf, String)> = Vec::new();
    for file in &c.changed {
        for f in s.shown_in(&cfg, file) {
            let k = (file.clone(), f.statement.clone());
            if !shown.contains(&k) {
                shown.push(k);
            }
        }
    }
    let hit = shown.iter().any(|(_, st)| c.hits.contains(st));
    Row {
        id: c.id.clone(),
        set: c.set.clone(),
        shown: shown.len(),
        hit,
    }
}

fn table(rows: &[Row]) -> (String, BTreeMap<&'static str, (usize, usize, f64)>) {
    let mut by = BTreeMap::new();
    let mut out =
        String::from("| set | cases | with a hit shown | shown a case |\n|---|---|---|---|\n");
    for k in SETS {
        let mine: Vec<&Row> = rows.iter().filter(|r| r.set == k).collect();
        let hits = mine.iter().filter(|r| r.hit).count();
        let per = mine.iter().map(|r| r.shown).sum::<usize>() as f64 / mine.len().max(1) as f64;
        out += &format!("| {k} | {} | {hits} | {per:.3} |\n", mine.len());
        by.insert(k, (mine.len(), hits, per));
    }
    out += "\n| case | set | shown | hit |\n|---|---|---|---|\n";
    for r in rows {
        out += &format!(
            "| {} | {} | {} | {} |\n",
            r.id,
            r.set,
            r.shown,
            if r.hit { "yes" } else { "" }
        );
    }
    (out, by)
}

#[test]
fn the_eval_reaches_its_passing_scores() {
    let only: Option<Vec<String>> = std::env::var("SCRY_EVAL_CASES")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.split(',').map(str::to_string).collect());
    let record = std::env::var("SCRY_RECORD_JEV").as_deref() == Ok("1");
    let checks: BTreeMap<String, String> =
        serde_json::from_value(read_json(&corpus().join("checks.json"))).unwrap();

    let mut cases = cases();
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    let mut rows = Vec::new();
    for c in cases
        .iter()
        .filter(|c| only.as_ref().is_none_or(|o| o.contains(&c.id)))
    {
        rows.push(run_case(c, &checks, record, only.is_some()));
    }
    let (text, by) = table(&rows);
    println!("{text}");
    if only.is_some() {
        return;
    }
    let (_, v, _) = by["V-must"];
    let (_, b, _) = by["B"];
    let (_, _, t) = by["T"];
    let (_, _, n) = by["N"];
    assert!(
        v >= 16,
        "V-must with a hit shown: {v} of 18, the passing score is 16"
    );
    assert!(
        b >= 6,
        "B with a hit shown: {b} of 8, the passing score is 6"
    );
    assert!(
        t <= 0.5 && n <= 0.5,
        "noise passing score missed: T {t}, N {n}"
    );
}
