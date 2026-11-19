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
//! `SCRY_RECORD=jev` (with `JEV_API_KEY`) asks Jev what is not recorded;
//! `SCRY_RECORD=all` also records the review of a case with no
//! `reviews/<case>.json` and the claim check of a statement not in
//! `checks.json`, through the live researcher (`researcher_for`). Outward
//! calls, run by hand, refused when `CI` is set.
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use scry::adapters::research_reviewer::ResearchReviewer;
use scry::adapters::wiring::{claim_checker, jev_for, researcher_for};
use scry::domain::budget::Caps;
use scry::domain::config::scry_config;
use scry::domain::findings::Findings;
use scry::domain::review_prompt::ReviewAsk;
use scry::domain::scope::ScopeRequest;
use scry::domain::steering::{Steering, parse_steering};
use scry::ports::claim_checker::ClaimChecker;
use scry::ports::reviewer::{Reviewed, Reviewer};
use scry::ports::run_log::{Quiet, RunLog};
use scry::usecases::review_change::{
    Progress, ReviewDeps, ReviewOutcome, ReviewRequest, review_change,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::SystemEnv;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
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
    let c = corpus_at(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/scry")
            .join(name),
    );
    assert_eq!(c.name, name, "{}: name", c.dir.display());
    c
}

/// The corpus in DIR, as its `corpus.toml` describes it.
fn corpus_at(dir: &Path) -> Corpus {
    let dir = dir.to_path_buf();
    let file = dir.join("corpus.toml");
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let f: CorpusFile = toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
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
    All,
}

/// `SCRY_RECORD`'s value RECORD, refused when CI is set: no test reaches the
/// network anywhere but a terminal with the keys.
fn record_mode_from(record: Option<&str>, ci: bool) -> Record {
    let mode = match record {
        None | Some("") => return Record::Nothing,
        Some("jev") => Record::Jev,
        Some("all") => Record::All,
        Some(other) => panic!("SCRY_RECORD={other}: expected jev or all"),
    };
    assert!(!ci, "SCRY_RECORD is refused when CI is set");
    mode
}

fn record_mode() -> Record {
    record_mode_from(
        std::env::var("SCRY_RECORD").ok().as_deref(),
        std::env::var_os("CI").is_some(),
    )
}

/// The live calls a recording makes for one case: the reviewer, the claim
/// checker and Jev (recording into the corpus's `jev/`).
struct Live {
    reviewer: Box<dyn Reviewer>,
    checker: Box<dyn ClaimChecker>,
    decider: Box<dyn Decider>,
}

/// Builds a case's live calls from the corpus, the case's project and view.
type LiveFor<'a> = &'a (dyn Fn(&Corpus, &Arc<Project>, &Arc<FilesView>) -> Live + Sync);

/// The live calls scry itself would make: the researcher on the project's
/// provider (`claude -p` on the subscription), the real claim checker with
/// Jev guarding, and Jev.
fn live(corpus: &Corpus, project: &Arc<Project>, view: &Arc<FilesView>) -> Live {
    let jev = jev_for(project, &SystemEnv).expect("SCRY_RECORD=all needs JEV_API_KEY");
    let researcher = researcher_for(project, &SystemEnv, view.clone()).expect("a researcher");
    let checker = claim_checker(
        researcher.clone(),
        Arc::new(jev.clone()),
        Fences::for_project(project).unwrap(),
        view.clone(),
        project.languages.clone(),
        Cancel::default(),
    );
    Live {
        reviewer: Box::new(ResearchReviewer { researcher }),
        checker,
        decider: Box::new(RecordedDecider::new(corpus.dir.join("jev")).recording_with(jev)),
    }
}

/// What a recording asked and spent.
#[derive(Default)]
struct Tally {
    reviews: usize,
    review_usd: f64,
    checks: usize,
    check_usd: f64,
}

fn verdict_word(v: &Verdict) -> &'static str {
    match v {
        Verdict::Right => "right",
        Verdict::Wrong => "wrong",
        Verdict::CannotTell => "cannot_tell",
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
/// when none are). A case with no recording asks LIVE and appends each pack
/// to `reviews/<case>.json` when recording, and fails naming the case and
/// pack otherwise.
struct CorpusReviewer<'a> {
    case: String,
    file: PathBuf,
    findings: Option<Vec<Value>>,
    live: Option<&'a dyn Reviewer>,
    tally: &'a Mutex<Tally>,
}

impl<'a> CorpusReviewer<'a> {
    fn for_case(
        corpus: &Corpus,
        id: &str,
        live: Option<&'a dyn Reviewer>,
        tally: &'a Mutex<Tally>,
    ) -> CorpusReviewer<'a> {
        let file = corpus.dir.join("reviews").join(format!("{id}.json"));
        let findings = file.exists().then(|| recorded_findings(&file));
        CorpusReviewer {
            case: id.to_string(),
            file,
            findings,
            live,
            tally,
        }
    }

    /// Appends one pack to the case's recording, packs sorted by their files.
    fn record(&self, files: Vec<String>, reviewed: &Reviewed) {
        let mut tally = self.tally.lock().unwrap();
        let mut rec = if self.file.exists() {
            read_json(&self.file)
        } else {
            json!({"packs": []})
        };
        let cost = reviewed.errand.cost_usd.unwrap_or(0.0);
        let packs = rec["packs"].as_array_mut().unwrap();
        packs.push(json!({"files": files, "reply": reviewed.reply, "cost_usd": cost}));
        packs.sort_by_key(|p| p["files"].to_string());
        std::fs::create_dir_all(self.file.parent().unwrap()).unwrap();
        std::fs::write(
            &self.file,
            serde_json::to_string_pretty(&rec).unwrap() + "\n",
        )
        .unwrap();
        tally.reviews += 1;
        tally.review_usd += cost;
    }
}

impl Reviewer for CorpusReviewer<'_> {
    fn review(
        &self,
        ask: &ReviewAsk,
        model: &str,
        cap_usd: f64,
        cancel: &Cancel,
    ) -> sorcerer_models::Result<Reviewed> {
        let files: BTreeSet<&Path> = ask.units.iter().map(|u| u.file.as_path()).collect();
        let Some(findings) = &self.findings else {
            let names: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
            let Some(live) = self.live else {
                panic!(
                    "case {}: no recorded review for the pack [{}] ({}); record it with SCRY_RECORD=all",
                    self.case,
                    names.join(", "),
                    self.file.display()
                );
            };
            let reviewed = live.review(ask, model, cap_usd, cancel)?;
            self.record(names, &reviewed);
            return Ok(reviewed);
        };
        let mine: Vec<&Value> = findings
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

/// Everything a corpus run shares between its cases.
struct Run<'a> {
    corpus: &'a Corpus,
    checks: Mutex<BTreeMap<String, String>>,
    record: Record,
    look: bool,
    live_for: LiveFor<'a>,
    tally: Mutex<Tally>,
}

fn run_case(run: &Run, c: &Case) -> Row {
    let corpus = run.corpus;
    let (record, look) = (run.record, run.look);
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
    let view = Arc::new(view);
    let cfg = scry_config(&project).unwrap();
    let live = (record == Record::All).then(|| (run.live_for)(corpus, &project, &view));
    let reviewer = CorpusReviewer::for_case(
        corpus,
        &c.id,
        live.as_ref().map(|l| &*l.reviewer),
        &run.tally,
    );
    let mut recorded = RecordedDecider::new(corpus.dir.join("jev"));
    if record == Record::Jev {
        // the live Jev, its ledger the temp project's
        recorded = recorded.recording_with(jev_for(&project, &SystemEnv).unwrap());
    }
    let decider: &dyn Decider = match &live {
        Some(l) => &*l.decider,
        None => &recorded,
    };
    let checker = |claim: &Claim| -> sorcerer_models::Result<Checked> {
        let key = collapse(&claim.statement);
        let known = run.checks.lock().unwrap().get(&key).cloned();
        if let (None, Some(l)) = (&known, &live) {
            let checked = l.checker.check(claim)?;
            let mut checks = run.checks.lock().unwrap();
            checks.insert(key, verdict_word(&checked.verdict).to_string());
            std::fs::write(
                corpus.dir.join("checks.json"),
                serde_json::to_string_pretty(&*checks).unwrap() + "\n",
            )
            .unwrap();
            let mut tally = run.tally.lock().unwrap();
            tally.checks += 1;
            tally.check_usd += checked
                .errands
                .iter()
                .filter_map(|e| e.cost_usd)
                .sum::<f64>();
            return Ok(checked);
        }
        let verdict = match known.as_deref() {
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
        inner: decider,
        case: &c.id,
    };
    let decider: &dyn Decider = if look { &looking } else { decider };
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
        view: &*view,
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
    run_corpus_with(corpus, only_cases(), record_mode(), &live)
}

/// `run_corpus` with its cases (ONLY), its recording and its live calls given.
fn run_corpus_with(
    corpus: &Corpus,
    only: Option<Vec<String>>,
    record: Record,
    live_for: LiveFor,
) -> (String, BySet) {
    let checks = serde_json::from_value(read_json(&corpus.dir.join("checks.json"))).unwrap();
    let jev_files = || std::fs::read_dir(corpus.dir.join("jev")).map_or(0, |d| d.count());
    let jev_before = jev_files();
    let run = Run {
        corpus,
        checks: Mutex::new(checks),
        record,
        look: only.is_some(),
        live_for,
        tally: Mutex::new(Tally::default()),
    };
    let mut cases = cases(corpus);
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    let mut rows = Vec::new();
    for c in cases
        .iter()
        .filter(|c| only.as_ref().is_none_or(|o| o.contains(&c.id)))
    {
        rows.push(run_case(&run, c));
        if record == Record::All {
            let t = run.tally.lock().unwrap();
            println!(
                "{}: recorded so far ${:.3}",
                c.id,
                t.review_usd + t.check_usd
            );
        }
    }
    let (text, by) = table(corpus, &rows);
    println!("{text}");
    if record != Record::Nothing {
        let t = run.tally.lock().unwrap();
        println!(
            "recorded: {} reviews ${:.3}, {} checks ${:.3}, {} jev answers",
            t.reviews,
            t.review_usd,
            t.checks,
            t.check_usd,
            jev_files() - jev_before
        );
    }
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

// ---- the record mode, on a fixture corpus (no network) ----

/// A fixture corpus at `CARGO_TARGET_TMPDIR/NAME`: a four-file Python base,
/// case `c1` changing `a.py`, case `c2` adding a 400-line function to each
/// of `c.py` and `d.py` (two packs), no recordings, no passing scores.
fn fixture_corpus(name: &str) -> Corpus {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let base = dir.join("base");
    for f in ["a.py", "b.py", "c.py", "d.py"] {
        support::write(&base, f, "def f():\n    return 1\n");
    }
    let big = format!(
        "\n\ndef big():\n{}    return 1\n",
        "    x = 1\n".repeat(400)
    );
    let cases: [(&str, &[(&str, String)]); 2] = [
        ("c1", &[("a.py", "def f():\n    return 2\n".to_string())]),
        (
            "c2",
            &[
                ("c.py", format!("def f():\n    return 1\n{big}")),
                ("d.py", format!("def f():\n    return 1\n{big}")),
            ],
        ),
    ];
    let mut labels = serde_json::Map::new();
    for (id, edits) in cases {
        let work = tempfile::TempDir::new().unwrap();
        copy_tree(&base, work.path());
        git(work.path(), &["init", "-q"]);
        git(work.path(), &["add", "-A"]);
        git(
            work.path(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e",
                "commit",
                "-qm",
                "b",
            ],
        );
        for (f, text) in edits {
            support::write(work.path(), f, text);
        }
        let diff = std::process::Command::new("git")
            .args(["diff"])
            .current_dir(work.path())
            .output()
            .unwrap()
            .stdout;
        support::write(
            &dir,
            &format!("cases/{id}.patch"),
            &String::from_utf8(diff).unwrap(),
        );
        let changed: Vec<&str> = edits.iter().map(|(f, _)| *f).collect();
        labels.insert(
            id.to_string(),
            json!({"set": "V-must", "hits": [], "changed": changed}),
        );
    }
    support::write(&dir, "labels.json", &Value::Object(labels).to_string());
    support::write(&dir, "checks.json", "{}");
    std::fs::create_dir_all(dir.join("jev")).unwrap();
    support::write(
        &dir,
        "scry.toml",
        "context = \"A fixture.\"\n\n[[rule]]\nid = \"small\"\ntext = \"Functions stay small.\"\n",
    );
    support::write(
        &dir,
        "corpus.toml",
        &format!("name = \"{name}\"\nlanguage = \"python\"\nsets = [\"V-must\"]\n"),
    );
    corpus_at(&dir)
}

/// A bug finding on FILE at LINE, as a review reply.
fn bug_reply(file: &str, line: u32, statement: &str) -> String {
    json!({
        "findings": [{
            "issue_type": "bug",
            "file": file,
            "lines": [line, line],
            "statement": statement,
            "rule": "",
            "evidence": [{"file": file, "lines": [line, line]}],
        }],
        "open": [],
    })
    .to_string()
}

/// Stubs for the live calls: a reviewer answering one bug a pack, a
/// checker calling every claim right at $0.01, a decider answering no.
fn stub_live(_: &Corpus, _: &Arc<Project>, _: &Arc<FilesView>) -> Live {
    let reviewer = support::StubReviewer::new()
        .reply(&["a.py"], &bug_reply("a.py", 2, "f returns 2, not 1."))
        .reply(&["c.py"], &bug_reply("c.py", 6, "big never uses x."))
        .reply(
            &["d.py"],
            &bug_reply("d.py", 6, "big in d.py never uses x."),
        );
    let checker = |_: &Claim| -> sorcerer_models::Result<Checked> {
        Ok(Checked {
            verdict: Verdict::Right,
            why: "stub".into(),
            citations: Vec::new(),
            cited: Vec::new(),
            escalated: false,
            errands: vec![errand("check", "haiku", 0.01)],
        })
    };
    Live {
        reviewer: Box::new(reviewer),
        checker: Box::new(checker),
        decider: Box::new(support::StubDecider::new()),
    }
}

#[test]
fn recording_writes_each_pack_and_the_verdict_word() {
    let c = fixture_corpus("eval-fixture-record");
    run_corpus_with(&c, None, Record::All, &stub_live);
    let packs = |id: &str| -> Vec<Vec<String>> {
        read_json(&c.dir.join("reviews").join(format!("{id}.json")))["packs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| serde_json::from_value(p["files"].clone()).unwrap())
            .collect()
    };
    assert_eq!(packs("c1"), vec![vec!["a.py".to_string()]]);
    assert_eq!(
        packs("c2"),
        vec![vec!["c.py".to_string()], vec!["d.py".to_string()]]
    );
    let checks: BTreeMap<String, String> =
        serde_json::from_value(read_json(&c.dir.join("checks.json"))).unwrap();
    assert_eq!(
        checks.get("f returns 2, not 1.").map(String::as_str),
        Some("right")
    );
    // a second run replays them: no review or check is asked again (the stub
    // Jev recorded nothing, so it stays live)
    struct Refuse;
    impl Reviewer for Refuse {
        fn review(
            &self,
            _: &ReviewAsk,
            _: &str,
            _: f64,
            _: &Cancel,
        ) -> sorcerer_models::Result<Reviewed> {
            panic!("a recorded review was asked again")
        }
    }
    let again = run_corpus_with(&c, None, Record::All, &|_, _, _| Live {
        reviewer: Box::new(Refuse),
        checker: Box::new(|_: &Claim| -> sorcerer_models::Result<Checked> {
            panic!("a recorded check was asked again")
        }),
        decider: Box::new(support::StubDecider::new()),
    });
    assert_eq!(again.1["V-must"].0, 2);
}

#[test]
#[should_panic(expected = "case c1: no recorded review for the pack [a.py]")]
fn a_miss_without_record_names_the_case_and_pack() {
    let c = fixture_corpus("eval-fixture-miss");
    run_corpus_with(&c, None, Record::Nothing, &stub_live);
}

#[test]
#[should_panic(expected = "SCRY_RECORD is refused when CI is set")]
fn record_is_refused_under_ci() {
    record_mode_from(Some("all"), true);
}
