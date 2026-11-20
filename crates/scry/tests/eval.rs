//! The eval: each corpus under `corpus/scry/<name>/` (its `corpus.toml`
//! names its language, sets and passing scores) run case by case through
//! whole passes with every model call replayed, scored against its passing
//! scores. One `#[test]` a corpus. The review calls replay the recorded
//! findings (split by the batch's files), the claim checks the recorded
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

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use scry::adapters::research_reviewer::ResearchReviewer;
use scry::adapters::wiring::{claim_checker, jev_for, researcher_for};
use scry::domain::budget::Caps;
use scry::domain::config::{ScryConfig, scry_config};
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
use sorcerer_models::domain::claim::{Checked, Claim, Escalation, Verdict};
use sorcerer_models::domain::decide::{Question, Weighed};
use sorcerer_models::ports::decider::Decider;
use support::{FixedClock, errand, git};
use tempfile::TempDir;

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
    /// True findings nobody planted: a person read each and judged it true.
    /// They are left out of the shown count.
    unplanted: Vec<String>,
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
            unplanted: c.get("unplanted").map_or_else(Vec::new, strings),
            changed: strings(&c["changed"])
                .into_iter()
                .map(PathBuf::from)
                .collect(),
        })
        .collect()
}

/// The findings of every batch recorded in `reviews/<case>.json`, in order.
fn recorded_findings(file: &Path) -> Vec<Value> {
    let rec = read_json(file);
    let batches = rec["batches"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: no batches", file.display()));
    let mut out = Vec::new();
    for batch in batches {
        let reply = batch["reply"].as_str().unwrap();
        if reply.trim().is_empty() {
            continue;
        }
        // read as the pass reads a reply: prose and a fenced block are allowed
        match scry::domain::review_prompt::json_in(reply) {
            Some(Value::Array(xs)) => out.extend(xs),
            Some(v) => out.extend(v["findings"].as_array().cloned().unwrap_or_default()),
            None => panic!("{}: no JSON in a recorded reply", file.display()),
        }
    }
    out
}

/// Every file some recorded batch in FILE holds.
fn recorded_files(file: &Path) -> BTreeSet<PathBuf> {
    read_json(file)["batches"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| p["files"].as_array().cloned().unwrap_or_default())
        .filter_map(|f| f.as_str().map(PathBuf::from))
        .collect()
}

/// The review call replayed: the case's recorded findings (over all its
/// batches) whose file is one of the batch's files, as one reply (an empty one
/// when none are). A case with no recording asks LIVE and appends each batch
/// to `reviews/<case>.json` when recording, and fails naming the case and
/// batch otherwise.
struct CorpusReviewer<'a> {
    case: String,
    file: PathBuf,
    findings: Option<Vec<Value>>,
    /// The files the recorded batches hold: a batch with a file outside them
    /// is a miss (a recording that stopped part way through the case).
    covered: BTreeSet<PathBuf>,
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
        let covered = if file.exists() {
            recorded_files(&file)
        } else {
            BTreeSet::new()
        };
        CorpusReviewer {
            case: id.to_string(),
            file,
            findings,
            covered,
            live,
            tally,
        }
    }

    /// Appends one batch to the case's recording, batches sorted by their files.
    fn record(&self, files: Vec<String>, reviewed: &Reviewed) {
        let mut tally = self.tally.lock().unwrap();
        let mut rec = if self.file.exists() {
            read_json(&self.file)
        } else {
            json!({"batches": []})
        };
        let cost = reviewed.errand.cost_usd.unwrap_or(0.0);
        let batches = rec["batches"].as_array_mut().unwrap();
        batches.push(json!({"files": files, "reply": reviewed.reply, "cost_usd": cost}));
        batches.sort_by_key(|p| p["files"].to_string());
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
        if let Some(findings) = self.recorded_for(&files) {
            return Ok(replayed_review(findings, &files, model));
        }
        let names: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
        let Some(live) = self.live else {
            // an error, not a panic: the pass fails the case after its log is written
            return Err(self.not_recorded(&names));
        };
        let reviewed = live.review(ask, model, cap_usd, cancel)?;
        self.record(names, &reviewed);
        Ok(reviewed)
    }
}

impl CorpusReviewer<'_> {
    /// The case's recorded findings, when its recording covers every one
    /// of FILES.
    fn recorded_for(&self, files: &BTreeSet<&Path>) -> Option<&Vec<Value>> {
        self.findings
            .as_ref()
            .filter(|_| files.iter().all(|f| self.covered.contains(*f)))
    }

    /// The miss of a batch of NAMES no recording covers.
    fn not_recorded(&self, names: &[String]) -> sorcerer_models::Error {
        sorcerer_models::Error::Reply {
            provider: "recorded".into(),
            why: format!(
                "case {}: no recorded review for the batch [{}] ({}); record it with SCRY_RECORD=all",
                self.case,
                names.join(", "),
                self.file.display()
            ),
        }
    }
}

/// FINDINGS on one of FILES, as one review reply.
fn replayed_review(findings: &[Value], files: &BTreeSet<&Path>, model: &str) -> Reviewed {
    let mine: Vec<&Value> = findings
        .iter()
        .filter(|f| {
            f["file"]
                .as_str()
                .is_some_and(|p| files.contains(Path::new(p)))
        })
        .collect();
    Reviewed {
        reply: json!({"findings": mine, "open": []}).to_string(),
        errand: errand("review", model, 0.04),
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
    unplanted: usize,
    to_read: Vec<String>,
}

/// What a case showed, sorted by what a person has said about each statement.
struct Counted {
    /// Shown statements that are not labelled unplanted.
    shown: usize,
    /// Whether a labelled hit was shown.
    hit: bool,
    /// Shown statements labelled unplanted (true, but nobody planted them).
    unplanted: usize,
    /// In a clean case (set T or N), shown statements in neither list: a
    /// person has yet to read them.
    to_read: Vec<String>,
}

/// Counts the statements a case showed. Example: in a set `N` case with
/// `unplanted = ["x"]` that showed `["x", "y"]`, `shown` is 1, `unplanted`
/// is 1 and `to_read` is `["y"]`.
fn count(c: &Case, shown: &[String]) -> Counted {
    let is_unplanted = |st: &&String| c.unplanted.contains(st);
    let clean_case = c.set == "T" || c.set == "N";
    Counted {
        shown: shown.iter().filter(|st| !is_unplanted(st)).count(),
        hit: shown.iter().any(|st| c.hits.contains(st)),
        unplanted: shown.iter().filter(is_unplanted).count(),
        to_read: shown
            .iter()
            .filter(|st| clean_case && !is_unplanted(st) && !c.hits.contains(*st))
            .cloned()
            .collect(),
    }
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

/// One case: its project, its replaying ports, the pass, its log written,
/// the pass ran without a failure, a look when asked, and its row.
fn run_case(run: &Run, c: &Case) -> Row {
    let corpus = run.corpus;
    let (_dir, project, view) = project_for_case(corpus, c);
    let cfg = scry_config(&project).unwrap();
    let live = (run.record == Record::All).then(|| (run.live_for)(corpus, &project, &view));
    let reviewer = CorpusReviewer::for_case(
        corpus,
        &c.id,
        live.as_ref().map(|l| &*l.reviewer),
        &run.tally,
    );
    let recorded = recorded_decider(corpus, run.record, &project);
    let decider: &dyn Decider = match &live {
        Some(l) => &*l.decider,
        None => &recorded,
    };
    let looking = Looking {
        inner: decider,
        case: &c.id,
    };
    let decider: &dyn Decider = if run.look { &looking } else { decider };
    let checker = CorpusChecker {
        run,
        live: live.as_ref(),
    };
    let (log_dir, log) = case_log(&c.id);
    let collect = |s: &str| log.borrow_mut().push_str(s);
    let log_to: &dyn RunLog = if log_dir.is_some() { &collect } else { &Quiet };
    let outcome = pass_over(&project, &view, &reviewer, decider, &checker, &cfg, log_to);
    // the log is written whatever happened, so a case that stops can be read
    write_case_log(log_dir.as_deref(), &c.id, &log.borrow());
    assert_ran(&c.id, outcome);

    let store = support::read_store(&project);
    if run.look {
        print_for_a_look(&c.id, &store);
    }
    let shown = shown_statements(&store, &cfg, &c.changed);
    let statements: Vec<String> = shown.into_iter().map(|(_, st)| st).collect();
    let counted = count(c, &statements);
    Row {
        id: c.id.clone(),
        set: c.set.clone(),
        shown: counted.shown,
        hit: counted.hit,
        unplanted: counted.unplanted,
        to_read: counted.to_read,
    }
}

/// The case's project: the corpus's base copied and committed, the case's
/// patch applied, the corpus's steering and a config naming its language.
fn project_for_case(corpus: &Corpus, case: &Case) -> (TempDir, Arc<Project>, Arc<FilesView>) {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    copy_tree(&corpus.dir.join("base"), root);
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    let patch = corpus.dir.join("cases").join(format!("{}.patch", case.id));
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
    (dir, project, Arc::new(view))
}

/// Jev's recording for CORPUS; under `SCRY_RECORD=jev` the live Jev answers
/// what is not recorded, its ledger the case's project's.
fn recorded_decider(corpus: &Corpus, record: Record, project: &Project) -> RecordedDecider {
    let recorded = RecordedDecider::new(corpus.dir.join("jev"));
    if record == Record::Jev {
        recorded.recording_with(jev_for(project, &SystemEnv).unwrap())
    } else {
        recorded
    }
}

/// The claim check replayed: the verdict `checks.json` holds for the
/// statement (none: cannot tell). When recording (`SCRY_RECORD=all`) a
/// statement it does not hold is asked live and written there.
struct CorpusChecker<'a> {
    run: &'a Run<'a>,
    live: Option<&'a Live>,
}

impl ClaimChecker for CorpusChecker<'_> {
    fn check(&self, claim: &Claim) -> sorcerer_models::Result<Checked> {
        let key = collapse(&claim.statement);
        let known = self.run.checks.lock().unwrap().get(&key).cloned();
        match (known, self.live) {
            (None, Some(live)) => {
                let checked = live.checker.check(claim)?;
                self.record_check(key, &checked);
                Ok(checked)
            }
            (known, _) => Ok(replayed(known.as_deref())),
        }
    }
}

impl CorpusChecker<'_> {
    /// CHECKED's verdict under KEY in `checks.json`, and its cost tallied.
    fn record_check(&self, key: String, checked: &Checked) {
        let mut checks = self.run.checks.lock().unwrap();
        checks.insert(key, verdict_word(&checked.verdict).to_string());
        std::fs::write(
            self.run.corpus.dir.join("checks.json"),
            serde_json::to_string_pretty(&*checks).unwrap() + "\n",
        )
        .unwrap();
        let mut tally = self.run.tally.lock().unwrap();
        tally.checks += 1;
        tally.check_usd += checked
            .errands
            .iter()
            .filter_map(|e| e.cost_usd)
            .sum::<f64>();
    }
}

/// A recorded verdict word as a check's result.
fn replayed(known: Option<&str>) -> Checked {
    let verdict = match known {
        Some("right") => Verdict::Right,
        Some("wrong") => Verdict::Wrong,
        _ => Verdict::CannotTell,
    };
    Checked {
        verdict,
        why: "1a's claim check".into(),
        citations: Vec::new(),
        cited: Vec::new(),
        escalation: Escalation::No,
        errands: Vec::new(),
    }
}

/// `SCRY_EVAL_LOG=DIR`, when set, and the case's log so far.
fn case_log(case: &str) -> (Option<PathBuf>, RefCell<String>) {
    (
        std::env::var_os("SCRY_EVAL_LOG").map(PathBuf::from),
        RefCell::new(format!("# scry eval case {case}\n")),
    )
}

/// LOG as `DIR/<case>.md`, when a DIR is set.
fn write_case_log(dir: Option<&Path>, case: &str, log: &str) {
    if let Some(d) = dir {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join(format!("{case}.md")), log).unwrap();
    }
}

/// One pass over the case's working tree through these ports, under the
/// corpus's caps.
fn pass_over(
    project: &Project,
    view: &FilesView,
    reviewer: &dyn Reviewer,
    decider: &dyn Decider,
    checker: &dyn ClaimChecker,
    cfg: &ScryConfig,
    log: &dyn RunLog,
) -> sorcerer_core::Result<ReviewOutcome> {
    let clock = FixedClock::at("2026-11-18T23:00:00Z".parse().unwrap());
    let progress = |_: &Progress| {};
    let ports = support::Ports::of(project);
    let cancel = Cancel::default();
    let deps = ReviewDeps {
        view,
        reviewer,
        decider,
        checker,
        store: &ports.store,
        ledger: &ports.ledger,
        settings: &ports.settings,
        clock: &clock,
        log,
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
    review_change(project, &req, &deps)
}

/// The pass ran, with no failed review call.
fn assert_ran(case: &str, outcome: sorcerer_core::Result<ReviewOutcome>) {
    match outcome {
        Ok(ReviewOutcome::Ran(r)) => assert!(
            r.failures.is_empty(),
            "case {case}: failures {:?}",
            r.failures
        ),
        Ok(ReviewOutcome::Skipped(why)) => panic!("case {case} skipped: {why}"),
        Err(e) => panic!("case {case}: {e}"),
    }
}

/// Each stored finding, a line, for a look (`SCRY_EVAL_CASES`).
fn print_for_a_look(case: &str, findings: &Findings) {
    for f in &findings.findings {
        let first: String = f.statement.chars().take(90).collect();
        println!(
            "{} {} {:?} {} confidence={:?} verdict={:?} unit={:?} lines={:?} {}",
            case,
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

/// The findings shown in each CHANGED file, once each, as (file, statement).
fn shown_statements(
    store: &Findings,
    cfg: &ScryConfig,
    changed: &[PathBuf],
) -> Vec<(PathBuf, String)> {
    let mut shown: Vec<(PathBuf, String)> = Vec::new();
    for file in changed {
        for f in store.shown_in(cfg, file) {
            let k = (file.clone(), f.statement.clone());
            if !shown.contains(&k) {
                shown.push(k);
            }
        }
    }
    shown
}

type BySet = BTreeMap<String, (usize, usize, f64)>;

fn table(corpus: &Corpus, rows: &[Row]) -> (String, BySet) {
    let mut by = BTreeMap::new();
    let mut out = format!(
        "# {}\n\n| set | cases | with a hit shown | shown a case | unplanted |\n|---|---|---|---|---|\n",
        corpus.name
    );
    for k in &corpus.sets {
        let mine: Vec<&Row> = rows.iter().filter(|r| &r.set == k).collect();
        let hits = mine.iter().filter(|r| r.hit).count();
        let unplanted = mine.iter().map(|r| r.unplanted).sum::<usize>();
        let per = mine.iter().map(|r| r.shown).sum::<usize>() as f64 / mine.len().max(1) as f64;
        out += &format!(
            "| {k} | {} | {hits} | {per:.3} | {unplanted} |\n",
            mine.len()
        );
        by.insert(k.clone(), (mine.len(), hits, per));
    }
    out += "\n| case | set | shown | hit | unplanted |\n|---|---|---|---|---|\n";
    for r in rows {
        out += &format!(
            "| {} | {} | {} | {} | {} |\n",
            r.id,
            r.set,
            r.shown,
            if r.hit { "yes" } else { "" },
            r.unplanted
        );
    }
    out += &to_read_text(rows);
    (out, by)
}

/// The statements a person has yet to read, one line each; empty when none.
fn to_read_text(rows: &[Row]) -> String {
    if rows.iter().all(|r| r.to_read.is_empty()) {
        return String::new();
    }
    let mut out = "\nto read (shown in a clean case, in no label list):\n".to_string();
    for r in rows {
        for st in &r.to_read {
            out += &format!("- {}: {st}\n", r.id);
        }
    }
    out
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
            stop_past_the_ceiling(&c.id, &run.tally.lock().unwrap());
        }
    }
    let (text, by) = table(corpus, &rows);
    println!("{text}");
    if record != Record::Nothing {
        println!(
            "{}",
            recorded_text(&run.tally.lock().unwrap(), jev_files() - jev_before)
        );
    }
    (text, by)
}

/// What a recording spent so far, after CASE; past `SCRY_RECORD_CEILING`
/// (default 5) the recording stops, keeping what it recorded.
fn stop_past_the_ceiling(case: &str, tally: &Tally) {
    let spent = tally.review_usd + tally.check_usd;
    println!("{case}: recorded so far ${spent:.3}");
    let ceiling: f64 = std::env::var("SCRY_RECORD_CEILING")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0);
    assert!(
        spent <= ceiling,
        "recording stopped: ${spent:.3} spent, past the ceiling ${ceiling}"
    );
}

/// What a recording asked and spent, and the Jev answers it added.
fn recorded_text(tally: &Tally, jev_answers: usize) -> String {
    format!(
        "recorded: {} reviews ${:.3}, {} checks ${:.3}, {} jev answers",
        tally.reviews, tally.review_usd, tally.checks, tally.check_usd, jev_answers
    )
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

#[test]
fn bistro_rs_reaches_its_passing_scores() {
    let c = corpus("bistro-rs");
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
/// of `c.py` and `d.py` (two batches), no recordings, no passing scores.
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

/// Stubs for the live calls: a reviewer answering one bug a batch, a
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
            escalation: Escalation::No,
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
fn recording_writes_each_batch_and_the_verdict_word() {
    let c = fixture_corpus("eval-fixture-record");
    run_corpus_with(&c, None, Record::All, &stub_live);
    let batches = |id: &str| -> Vec<Vec<String>> {
        read_json(&c.dir.join("reviews").join(format!("{id}.json")))["batches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| serde_json::from_value(p["files"].clone()).unwrap())
            .collect()
    };
    assert_eq!(batches("c1"), vec![vec!["a.py".to_string()]]);
    assert_eq!(
        batches("c2"),
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
#[should_panic(expected = "case c1: no recorded review for the batch [a.py]")]
fn a_miss_without_record_names_the_case_and_batch() {
    let c = fixture_corpus("eval-fixture-miss");
    run_corpus_with(&c, None, Record::Nothing, &stub_live);
}

#[test]
#[should_panic(expected = "SCRY_RECORD is refused when CI is set")]
fn record_is_refused_under_ci() {
    record_mode_from(Some("all"), true);
}

#[test]
fn a_partly_recorded_case_asks_for_the_missing_batch() {
    let c = fixture_corpus("eval-fixture-partial");
    run_corpus_with(&c, None, Record::All, &stub_live);
    // as if batch [c.py]'s live call had failed: c2 holds only batch [d.py]
    let file = c.dir.join("reviews/c2.json");
    let mut rec = read_json(&file);
    rec["batches"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p["files"] != json!(["c.py"]));
    std::fs::write(&file, rec.to_string()).unwrap();
    // a recording asks for that batch again (not an empty replay) and keeps [d.py]
    run_corpus_with(&c, None, Record::All, &stub_live);
    let batches: Vec<Value> = read_json(&file)["batches"].as_array().unwrap().clone();
    let files: Vec<&Value> = batches.iter().map(|p| &p["files"]).collect();
    assert_eq!(files, vec![&json!(["c.py"]), &json!(["d.py"])]);
}

// ---- unplanted labels and "to read" ----

fn case_with(set: &str, hits: &[&str], unplanted: &[&str]) -> Case {
    let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
    Case {
        id: "k".into(),
        set: set.into(),
        hits: own(hits),
        unplanted: own(unplanted),
        changed: Vec::new(),
    }
}

fn own(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn an_unplanted_finding_is_left_out_of_the_count() {
    let c = count(&case_with("N", &[], &["x"]), &own(&["x", "y"]));
    assert_eq!((c.shown, c.unplanted, c.hit), (1, 1, false));
    assert_eq!(c.to_read, own(&["y"]));
}

#[test]
fn a_planted_set_has_nothing_to_read() {
    let c = count(&case_with("V-must", &["h"], &[]), &own(&["h", "z"]));
    assert!(c.hit);
    assert_eq!(c.shown, 2);
    assert!(c.to_read.is_empty());
}

#[test]
fn the_table_has_the_unplanted_column_and_the_to_read_list() {
    let corpus = fixture_corpus("eval-fixture-table");
    let row = |id: &str, set: &str, to_read: &[&str]| Row {
        id: id.into(),
        set: set.into(),
        shown: 1,
        hit: false,
        unplanted: 0,
        to_read: own(to_read),
    };
    let (text, _) = table(
        &corpus,
        &[row("n9", "V-must", &["y"]), row("n8", "V-must", &[])],
    );
    assert!(text.contains("| unplanted |"));
    assert!(text.contains("to read (shown in a clean case, in no label list):"));
    assert!(text.contains("- n9: y"));
    let (text, _) = table(&corpus, &[row("n8", "V-must", &[])]);
    assert!(!text.contains("to read"));
}
