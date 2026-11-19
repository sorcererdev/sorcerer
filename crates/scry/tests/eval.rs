//! The eval: each corpus under `corpus/scry/<name>/` (its `corpus.toml`
//! names its language, sets and passing scores) run case by case through
//! whole passes with every model call replayed, scored against its passing
//! scores. One `#[test]` a corpus. The review calls replay the recorded
//! findings (split by the pack's files), the claim checks the recorded
//! verdicts, and Jev its recording under the corpus's `jev/`.
//!
//! `SCRY_EVAL_CASES=b01,vm02` runs only those cases and prints their
//! findings (the passing scores are then not asserted: a look, not a measure).
//! `SCRY_EVAL_LOG=DIR` writes each case's scope and pass trace (the run
//! log's middle, as `scry -v` would stream it) to `DIR/<id>.md`.
//! `SCRY_RECORD=jev` (with `JEV_API_KEY`) asks Jev what is not recorded:
//! an outward call, run by hand, never in CI.
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use scry::domain::budget::Caps;
use scry::domain::config::scry_config;
use scry::domain::findings::Findings;
use scry::domain::review_prompt::ReviewAsk;
use scry::domain::scope::ScopeRequest;
use scry::domain::steering::{Steering, parse_steering};
use scry::ports::reviewer::{Reviewed, Reviewer};
use scry::ports::run_log::{Quiet, RunLog};
use scry::usecases::review_change::{
    Progress, ReviewDeps, ReviewOutcome, ReviewRequest, review_change,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::SystemEnv;
use sorcerer_models::adapters::jev::question_key;
use sorcerer_models::adapters::recorded_decider::RecordedDecider;
use sorcerer_models::domain::claim::{Checked, Claim, Verdict};
use sorcerer_models::domain::decide::{Question, Weighed};
use sorcerer_models::ports::decider::Decider;
use support::{FixedClock, errand, git};

/// A corpus directory and what its `corpus.toml` says.
struct Corpus {
    dir: PathBuf,
    name: String,
    language: String,
    sets: Vec<String>,
    passing_scores: Option<PassingScores>,
}

#[derive(Deserialize)]
struct PassingScores {
    v_must_min: usize,
    b_min: usize,
    t_mean_max: f64,
    n_mean_max: f64,
}

#[derive(Deserialize)]
struct CorpusFile {
    name: String,
    language: String,
    sets: Vec<String>,
    passing_scores: Option<PassingScores>,
}

/// The corpus at `corpus/scry/<name>/`.
fn corpus(name: &str) -> Corpus {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/scry")
        .join(name);
    let file = dir.join("corpus.toml");
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let f: CorpusFile = toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    assert_eq!(f.name, name, "{}: name", file.display());
    Corpus {
        dir,
        name: f.name,
        language: f.language,
        sets: f.sets,
        passing_scores: f.passing_scores,
    }
}

/// What `SCRY_RECORD` asks for: nothing (replay only), Jev's answers, or
/// everything that has no recording.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Record {
    Nothing,
    Jev,
}

fn record_mode() -> Record {
    match std::env::var("SCRY_RECORD").as_deref() {
        Err(_) | Ok("") => Record::Nothing,
        Ok("jev") => Record::Jev,
        Ok("all") => panic!("SCRY_RECORD=all: not yet"),
        Ok(other) => panic!("SCRY_RECORD={other}: expected jev or all"),
    }
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

fn cases(corpus: &Corpus) -> Vec<Case> {
    let labels = read_json(&corpus.dir.join("labels.json"));
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

/// The findings of every pack recorded in `reviews/<case>.json`, in order.
fn recorded_findings(file: &Path) -> Vec<Value> {
    let rec = read_json(file);
    let packs = rec["packs"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: no packs", file.display()));
    let mut out = Vec::new();
    for pack in packs {
        let reply = pack["reply"].as_str().unwrap();
        if reply.trim().is_empty() {
            continue;
        }
        let reply: Value =
            serde_json::from_str(reply).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        out.extend(reply["findings"].as_array().cloned().unwrap_or_default());
    }
    out
}

/// The review call replayed: the case's recorded findings (over all its
/// packs) whose file is one of the pack's files, as one reply (an empty one
/// when none are).
struct CorpusReviewer {
    findings: Vec<Value>,
}

impl CorpusReviewer {
    fn for_case(corpus: &Corpus, id: &str) -> CorpusReviewer {
        let file = corpus.dir.join("reviews").join(format!("{id}.json"));
        assert!(
            file.exists(),
            "case {id}: no recorded review ({}); record it with SCRY_RECORD=all",
            file.display()
        );
        CorpusReviewer {
            findings: recorded_findings(&file),
        }
    }
}

impl Reviewer for CorpusReviewer {
    fn review(
        &self,
        ask: &ReviewAsk,
        model: &str,
        _cap_usd: f64,
        _cancel: &Cancel,
    ) -> sorcerer_models::Result<Reviewed> {
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
        Ok(Reviewed {
            reply: json!({"findings": mine, "open": []}).to_string(),
            errand: errand("review", model, 0.04),
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

fn run_case(
    corpus: &Corpus,
    c: &Case,
    checks: &BTreeMap<String, String>,
    record: Record,
    look: bool,
) -> Row {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    copy_tree(&corpus.dir.join("base"), root);
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    let patch = corpus.dir.join("cases").join(format!("{}.patch", c.id));
    git(root, &["apply", patch.to_str().unwrap()]);
    support::write(
        root,
        ".sorcerer/scry.toml",
        &std::fs::read_to_string(corpus.dir.join("scry.toml")).unwrap(),
    );
    support::write(
        root,
        ".sorcerer/config.toml",
        &format!(
            "[languages]\nnames = [\"{}\"]\n\n[scry]\nbudget_per_pass = 5.0\nbudget_per_day = 1000.0\n",
            corpus.language
        ),
    );

    let (project, view) = support::project_with_vendored_grammars(root);
    let cfg = scry_config(&project).unwrap();
    let reviewer = CorpusReviewer::for_case(corpus, &c.id);
    let mut decider = RecordedDecider::new(corpus.dir.join("jev"));
    if record != Record::Nothing {
        // the live Jev, its ledger the temp project's
        decider =
            decider.recording_with(scry::adapters::wiring::jev_for(&project, &SystemEnv).unwrap());
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
    let log = std::cell::RefCell::new(format!("# scry eval case {}\n", c.id));
    let collect = |s: &str| log.borrow_mut().push_str(s);
    let log_to: &dyn RunLog = if log_dir.is_some() { &collect } else { &Quiet };
    let ports = support::Ports::of(&project);
    let cancel = Cancel::default();
    let deps = ReviewDeps {
        view: &view,
        reviewer: &reviewer,
        decider,
        checker: &checker,
        store: &ports.store,
        ledger: &ports.ledger,
        settings: &ports.settings,
        clock: &clock,
        log: log_to,
        progress: &progress,
        cancel: &cancel,
    };
    let req = ReviewRequest {
        caps: Some(Caps {
            pass: Some(cfg.budget_per_pass),
            day: cfg.budget_per_day,
            budget: None,
        }),
        ..ReviewRequest::of(ScopeRequest::Working)
    };
    let outcome = review_change(&project, &req, &deps);
    // the log is written whatever happened, so a case that stops can be read
    if let Some(d) = &log_dir {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join(format!("{}.md", c.id)), log.borrow().as_str()).unwrap();
    }
    match outcome {
        Ok(ReviewOutcome::Ran(r)) => assert!(
            r.failures.is_empty(),
            "case {}: failures {:?}",
            c.id,
            r.failures
        ),
        Ok(ReviewOutcome::Skipped(why)) => panic!("case {} skipped: {why}", c.id),
        Err(e) => panic!("case {}: {e}", c.id),
    }

    let s: Findings = support::read_store(&project);
    if look {
        for f in &s.findings {
            let first: String = f.statement.chars().take(90).collect();
            println!(
                "{} {} {:?} {} confidence={:?} verdict={:?} unit={:?} lines={:?} {}",
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

type BySet = BTreeMap<String, (usize, usize, f64)>;

fn table(corpus: &Corpus, rows: &[Row]) -> (String, BySet) {
    let mut by = BTreeMap::new();
    let mut out = format!(
        "# {}\n\n| set | cases | with a hit shown | shown a case |\n|---|---|---|---|\n",
        corpus.name
    );
    for k in &corpus.sets {
        let mine: Vec<&Row> = rows.iter().filter(|r| &r.set == k).collect();
        let hits = mine.iter().filter(|r| r.hit).count();
        let per = mine.iter().map(|r| r.shown).sum::<usize>() as f64 / mine.len().max(1) as f64;
        out += &format!("| {k} | {} | {hits} | {per:.3} |\n", mine.len());
        by.insert(k.clone(), (mine.len(), hits, per));
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

/// The cases `SCRY_EVAL_CASES` names, when it is set: a look, not a measure.
fn only_cases() -> Option<Vec<String>> {
    std::env::var("SCRY_EVAL_CASES")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.split(',').map(str::to_string).collect())
}

/// Runs CORPUS's cases (those `SCRY_EVAL_CASES` names, when it is set) and
/// returns its table and the numbers by set; the table is printed.
fn run_corpus(corpus: &Corpus) -> (String, BySet) {
    let only = only_cases();
    let record = record_mode();
    let checks: BTreeMap<String, String> =
        serde_json::from_value(read_json(&corpus.dir.join("checks.json"))).unwrap();

    let mut cases = cases(corpus);
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    let mut rows = Vec::new();
    for c in cases
        .iter()
        .filter(|c| only.as_ref().is_none_or(|o| o.contains(&c.id)))
    {
        rows.push(run_case(corpus, c, &checks, record, only.is_some()));
    }
    let (text, by) = table(corpus, &rows);
    println!("{text}");
    (text, by)
}

/// CORPUS's numbers against its passing scores; a look (`SCRY_EVAL_CASES`)
/// or a recording (`SCRY_RECORD`) asserts nothing, and a corpus with no
/// passing scores fails.
fn assert_passing_scores(corpus: &Corpus, by: &BySet) {
    if only_cases().is_some() || record_mode() != Record::Nothing {
        return;
    }
    let Some(p) = &corpus.passing_scores else {
        panic!("{}: no passing scores yet (corpus.toml)", corpus.name);
    };
    let (vn, v, _) = by["V-must"];
    let (bn, b, _) = by["B"];
    let (_, _, t) = by["T"];
    let (_, _, n) = by["N"];
    let name = &corpus.name;
    assert!(
        v >= p.v_must_min,
        "{name}: V-must with a hit shown: {v} of {vn}, the passing score is {}",
        p.v_must_min
    );
    assert!(
        b >= p.b_min,
        "{name}: B with a hit shown: {b} of {bn}, the passing score is {}",
        p.b_min
    );
    assert!(
        t <= p.t_mean_max && n <= p.n_mean_max,
        "{name}: noise passing score missed: T {t} (at most {}), N {n} (at most {})",
        p.t_mean_max,
        p.n_mean_max
    );
}

#[test]
fn bistro_reaches_its_passing_scores() {
    let c = corpus("bistro");
    let (_, by) = run_corpus(&c);
    assert_passing_scores(&c, &by);
}

/// Every rule a recorded reply cites is one of the corpus steering's ids
/// (the replies were recorded with these ids; nothing was mapped since).
#[test]
fn the_recorded_replies_cite_the_corpus_rule_ids() {
    let bistro = corpus("bistro");
    let file = bistro.dir.join("scry.toml");
    let raw = parse_steering(&std::fs::read_to_string(&file).unwrap(), &file).unwrap();
    let ids: BTreeSet<&str> = raw.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), 13);
    let steering = Steering {
        context: raw.context.clone(),
        includes: Vec::new(),
        rules: raw.rules.clone(),
    };
    assert!(steering.over_cap().is_none());
    let mut cited = 0;
    for e in std::fs::read_dir(bistro.dir.join("reviews")).unwrap() {
        let path = e.unwrap().path();
        for f in &recorded_findings(&path) {
            assert!(f.get("kind").is_none(), "{}", path.display());
            let rule = f["rule"].as_str().unwrap_or("").trim();
            if !rule.is_empty() {
                assert!(ids.contains(rule), "{}: {rule}", path.display());
                cited += 1;
            }
        }
    }
    assert_eq!(cited, 33);
}
