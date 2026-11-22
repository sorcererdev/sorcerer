//! The eval: each corpus under `corpus/conjure/<name>/` run case by case through
//! `conjure_change` with the writer's reply replayed from `replies/<id>.json`, and scored
//! against the corpus's passing scores. One `#[test]` a corpus.
//!
//! `CONJURE_EVAL_CASES=a,b` runs only those cases (the scores are then not asserted: a look,
//! not a measure). `CONJURE_RECORD=all` asks the live writer (`[conjure] writer` on the
//! project's provider) for a case with no reply and writes it to `replies/<id>.json`; it stops
//! once what it recorded costs `CONJURE_RECORD_CEILING` dollars (default 0.50), keeping what it
//! wrote. Outward calls are run by hand, and refused when `CI` is set.
mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use conjure::adapters::project_context::FilesContext;
use conjure::adapters::provider_writer::ProviderWriter;
use conjure::adapters::system_clock::SystemClock;
use conjure::domain::config::conjure_config;
use conjure::domain::edits::NameCase;
use conjure::domain::language_tables::tests_path;
use conjure::domain::record::Attempt;
use conjure::ports::writer::{Writer, Written};
use conjure::usecases::conjure_change::{
    ChangeDeps, ChangeRequest, ConjuredChange, conjure_change,
};
use serde::{Deserialize, Serialize};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::SystemEnv;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::Span;
use sorcerer_models::adapters::providers::default_provider_for;
use support::{FixedClock, git, project_with_vendored_grammars, test_cache};
use tempfile::TempDir;

/// What `CONJURE_RECORD` asks for: nothing (replay only), or the live writer for the cases with
/// no recorded reply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Record {
    Off,
    All,
}

/// `CONJURE_RECORD`'s value RECORD, refused when CI is set: no test reaches the model there.
fn record_mode_from(record: Option<&str>, ci: bool) -> Record {
    match record {
        None => return Record::Off,
        Some("all") => {}
        Some(other) => panic!("CONJURE_RECORD={other}: expected all"),
    }
    assert!(!ci, "CONJURE_RECORD is refused when CI is set");
    Record::All
}

/// The mode the real corpora run in.
fn record_mode() -> Record {
    record_mode_from(
        std::env::var("CONJURE_RECORD").ok().as_deref(),
        std::env::var_os("CI").is_some(),
    )
}

/// What a corpus's `corpus.toml` says.
#[derive(Deserialize)]
struct CorpusFile {
    name: String,
    language: String,
    /// The `[conjure] writer` alias the replies were recorded with.
    writer: String,
    passing_scores: Option<PassingScores>,
}

#[derive(Deserialize)]
struct PassingScores {
    /// Absent until the corpus has been recorded and labelled (Rust's first recording sets it).
    forward_min: Option<usize>,
    parses_min: usize,
    in_scope_min: usize,
    no_change_on_green_min: usize,
}

/// One `cases/<id>.toml`.
#[derive(Deserialize)]
struct CaseFile {
    file: String,
    line: u32,
    end_line: Option<u32>,
    kind: String,
    /// What a person would call the right next change; read by whoever labels the reply.
    #[allow(dead_code)]
    expect: String,
    finding: Option<String>,
    #[serde(default)]
    also_in_scope: Vec<String>,
}

#[derive(Deserialize)]
struct Labels {
    forward: BTreeSet<String>,
}

/// A recorded reply: the writer's text and what the call cost.
#[derive(Serialize, Deserialize)]
struct RecordedReply {
    model: String,
    text: String,
    cost_usd: f64,
    seconds: f64,
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn ceiling_usd() -> f64 {
    std::env::var("CONJURE_RECORD_CEILING")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.50)
}

/// The case ids of a corpus, sorted; `CONJURE_EVAL_CASES` narrows them.
fn case_ids(dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(dir.join("cases"))
        .unwrap()
        .filter_map(|e| e.unwrap().file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".toml").map(str::to_string))
        .collect();
    ids.sort();
    if let Ok(only) = std::env::var("CONJURE_EVAL_CASES") {
        ids.retain(|id| only.split(',').any(|o| o == id));
    }
    ids
}

/// Answers a case from `replies/<id>.json`; a case with none is asked of the live writer when
/// recording, and its reply is written down.
struct CorpusWriter<'a> {
    id: String,
    replies: PathBuf,
    model: String,
    live: Option<&'a ProviderWriter>,
    /// Dollars the recording has spent so far, over every case.
    spent: &'a Mutex<f64>,
}

impl Writer for CorpusWriter<'_> {
    fn write(&self, situation: &str, cancel: &Cancel) -> sorcerer_models::Result<Written> {
        let file = self.replies.join(format!("{}.json", self.id));
        if file.exists() {
            return Ok(self.replayed(&file));
        }
        let Some(live) = self.live else {
            panic!(
                "case {}: no recorded reply ({}); record it with CONJURE_RECORD=all",
                self.id,
                file.display()
            );
        };
        let spent = *self.spent.lock().unwrap();
        assert!(
            spent < ceiling_usd(),
            "CONJURE_RECORD_CEILING reached (${spent:.4}); what is recorded is kept, run again for the rest"
        );
        let written = live.write(situation, cancel)?;
        self.record(&file, &written);
        Ok(written)
    }

    fn model(&self) -> &str {
        &self.model
    }
}

impl CorpusWriter<'_> {
    fn replayed(&self, file: &Path) -> Written {
        let text = std::fs::read_to_string(file).unwrap();
        let reply: RecordedReply =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        Written {
            text: reply.text,
            errand: Errand {
                at: Utc::now(),
                product: "conjure".into(),
                verb: "conjure".into(),
                model: reply.model,
                provider: "recorded".into(),
                cost_usd: Some(reply.cost_usd),
                input_tokens: 0,
                output_tokens: 0,
                seconds: reply.seconds,
                ok: true,
                note: None,
            },
        }
    }

    fn record(&self, file: &Path, written: &Written) {
        let cost_usd = written.errand.cost_usd.unwrap_or(0.0);
        *self.spent.lock().unwrap() += cost_usd;
        let reply = RecordedReply {
            model: written.errand.model.clone(),
            text: written.text.clone(),
            cost_usd,
            seconds: written.errand.seconds,
        };
        std::fs::create_dir_all(&self.replies).unwrap();
        std::fs::write(file, serde_json::to_string_pretty(&reply).unwrap() + "\n").unwrap();
        println!(
            "recorded {}: ${cost_usd:.4}, {:.1} s (total ${:.4})",
            self.id,
            written.errand.seconds,
            self.spent.lock().unwrap()
        );
    }
}

/// One case's result: what was conjured and how it scores.
#[derive(Debug, Clone)]
struct Outcome {
    id: String,
    forward: bool,
    parses: bool,
    in_scope: bool,
    /// For a green case: it was answered no change, or changed only its test file.
    green_answered_well: Option<bool>,
}

/// Every case of the corpus at DIR, conjured over its own copy of the project.
fn run_corpus(dir: &Path, record: Record) -> Vec<Outcome> {
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    let labels: Labels = serde_json::from_str(
        &std::fs::read_to_string(dir.join("labels.json"))
            .unwrap_or_else(|_| r#"{"forward": []}"#.into()),
    )
    .unwrap();
    let spent = Mutex::new(0.0);
    case_ids(dir)
        .into_iter()
        .map(|id| {
            run_case(
                dir,
                &corpus,
                &id,
                record,
                &spent,
                labels.forward.contains(&id),
            )
        })
        .collect()
}

/// Runs the corpus, prints its table and asserts its passing scores. A recording, or a run of
/// chosen cases, asserts nothing: it is a look, not a measure.
fn check_corpus(dir: &Path, record: Record) {
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    let outcomes = run_corpus(dir, record);
    let scores = Scores::of(&outcomes);
    println!("# {}\n{}", corpus.name, scores.text());
    if record == Record::All || std::env::var_os("CONJURE_EVAL_CASES").is_some() {
        return;
    }
    let passing = corpus
        .passing_scores
        .expect("corpus.toml has no [passing_scores]");
    scores.assert_reaches(&passing);
}

/// The four counts of a corpus's outcomes.
struct Scores {
    cases: usize,
    forward: usize,
    parses: usize,
    in_scope: usize,
    green_answered_well: usize,
    green_cases: usize,
}

impl Scores {
    fn of(outcomes: &[Outcome]) -> Scores {
        let count = |keep: &dyn Fn(&Outcome) -> bool| outcomes.iter().filter(|o| keep(o)).count();
        Scores {
            cases: outcomes.len(),
            forward: count(&|o| o.forward),
            parses: count(&|o| o.parses),
            in_scope: count(&|o| o.in_scope),
            green_answered_well: count(&|o| o.green_answered_well == Some(true)),
            green_cases: count(&|o| o.green_answered_well.is_some()),
        }
    }

    fn text(&self) -> String {
        format!(
            "forward {}/{}\nparses {}/{}\nin_scope {}/{}\nno_change_on_green {}/{}",
            self.forward,
            self.cases,
            self.parses,
            self.cases,
            self.in_scope,
            self.cases,
            self.green_answered_well,
            self.green_cases
        )
    }

    fn assert_reaches(&self, passing: &PassingScores) {
        let forward_min = passing
            .forward_min
            .expect("corpus.toml has no forward_min: set it from the first recording");
        let rows = [
            ("forward", self.forward, forward_min),
            ("parses", self.parses, passing.parses_min),
            ("in_scope", self.in_scope, passing.in_scope_min),
            (
                "no_change_on_green",
                self.green_answered_well,
                passing.no_change_on_green_min,
            ),
        ];
        for (name, got, min) in rows {
            assert!(
                got >= min,
                "{name}: {got} of {}, passing is {min}",
                self.cases
            );
        }
    }
}

/// A corpus's project for one case: the base, committed, then the person's edit uncommitted,
/// and scry's findings when the case has them.
fn project_of_case(dir: &Path, id: &str) -> TempDir {
    let project = TempDir::new().unwrap();
    copy_tree(&dir.join("base"), project.path());
    git(project.path(), &["init", "-q", "-b", "main"]);
    git(project.path(), &["config", "user.name", "case"]);
    git(
        project.path(),
        &["config", "user.email", "case@example.com"],
    );
    git(project.path(), &["add", "-A"]);
    git(project.path(), &["commit", "-q", "-m", "base"]);
    let patch = dir.join(format!("cases/{id}.patch"));
    if patch.exists() {
        git(project.path(), &["apply", patch.to_str().unwrap()]);
    }
    let findings = dir.join(format!("cases/{id}.findings.json"));
    if findings.exists() {
        let to = project.path().join(".sorcerer/state/scry/findings.json");
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(findings, to).unwrap();
    }
    std::fs::write(project.path().join(".git/info/exclude"), ".sorcerer/\n").unwrap();
    project
}

fn run_case(
    dir: &Path,
    corpus: &CorpusFile,
    id: &str,
    record: Record,
    spent: &Mutex<f64>,
    forward: bool,
) -> Outcome {
    let case: CaseFile = read_toml(&dir.join(format!("cases/{id}.toml")));
    let project_dir = project_of_case(dir, id);
    let (project, view) = project_with_vendored_grammars(project_dir.path());
    assert!(
        project
            .languages
            .for_path(Path::new(&case.file))
            .is_some_and(|l| l.name == corpus.language),
        "case {id}: {} is not {}",
        case.file,
        corpus.language
    );
    let needs_asking = !dir.join(format!("replies/{id}.json")).exists();
    let live =
        (record == Record::All && needs_asking).then(|| live_writer(&project, &corpus.writer));
    let writer = CorpusWriter {
        id: id.into(),
        replies: dir.join("replies"),
        model: corpus.writer.clone(),
        live: live.as_ref(),
        spent,
    };
    let context = FilesContext {
        project: project.clone(),
        view: std::sync::Arc::new(FilesView::new(project.clone(), test_cache())),
        cache: test_cache(),
        fences: Fences::for_project(&project).unwrap(),
    };
    let config = conjure_config(&project).unwrap();
    let fences = Fences::for_project(&project).unwrap();
    let ledger = JsonlLedger::for_project(&project).unwrap();
    let clock = FixedClock::at("2026-11-22T12:00:00Z".parse().unwrap());
    let cancel = Cancel::default();
    let deps = ChangeDeps {
        project: &project,
        view: &view,
        context: &context,
        fences: &fences,
        config: &config,
        writer: &writer,
        ledger: &ledger,
        clock: &clock,
        cancel: &cancel,
        name_case: NameCase::Matters,
    };
    let request = ChangeRequest {
        file: PathBuf::from(&case.file),
        line: case.line,
        selection: case.end_line.map(|end| Span {
            start: case.line,
            end,
        }),
        finding: case.finding.clone(),
        attempt: Attempt::First,
        situation: None,
    };
    let conjured = conjure_change(&request, &deps)
        .unwrap_or_else(|refused| panic!("case {id}: {}", refused.text()));
    let outcome = score(id, &case, &project, &conjured, forward);
    println!("{}", outcome_text(&outcome, &case.kind, &conjured));
    outcome
}

/// The writer of a recording: `[conjure] writer`'s alias on the project's provider.
fn live_writer(project: &sorcerer_core::domain::project::Project, alias: &str) -> ProviderWriter {
    let (provider, _) = default_provider_for(project, &SystemEnv)
        .expect("CONJURE_RECORD=all needs claude on PATH (or a key)");
    ProviderWriter {
        provider,
        model: alias.into(),
        ledger: std::sync::Arc::new(JsonlLedger::for_project(project).unwrap()),
        clock: std::sync::Arc::new(SystemClock),
    }
}

/// How a conjured change scores on the four counts.
fn score(
    id: &str,
    case: &CaseFile,
    project: &sorcerer_core::domain::project::Project,
    conjured: &ConjuredChange,
    forward: bool,
) -> Outcome {
    let allowed = scope_of(case, project);
    let changed: Vec<String> = conjured
        .files
        .iter()
        .map(|f| forward_slashes(&f.path))
        .collect();
    let green_answered_well = (case.kind == "green").then(|| {
        let own_tests = tests_of(case, project);
        changed.iter().all(|path| Some(path) == own_tests.as_ref())
    });
    Outcome {
        id: id.into(),
        forward,
        parses: conjured.files.iter().all(|f| f.parse_errors.is_empty()),
        in_scope: changed.iter().all(|path| allowed.contains(path)),
        green_answered_well,
    }
}

/// The unit's file, its test file, and what the case adds.
fn scope_of(
    case: &CaseFile,
    project: &sorcerer_core::domain::project::Project,
) -> BTreeSet<String> {
    let mut allowed: BTreeSet<String> = case.also_in_scope.iter().cloned().collect();
    allowed.insert(case.file.clone());
    allowed.extend(tests_of(case, project));
    allowed
}

fn tests_of(case: &CaseFile, project: &sorcerer_core::domain::project::Project) -> Option<String> {
    let file = Path::new(&case.file);
    let language = project.languages.for_path(file)?;
    let tests = tests_path(language, file).ok()??;
    Some(forward_slashes(&tests))
}

/// One line of the table printed under `--nocapture`.
fn outcome_text(outcome: &Outcome, kind: &str, conjured: &ConjuredChange) -> String {
    let files: Vec<String> = conjured
        .files
        .iter()
        .map(|f| forward_slashes(&f.path))
        .collect();
    format!(
        "{:<20} {:<9} forward={} parses={} in_scope={} files=[{}] dropped={} goal={}",
        outcome.id,
        kind,
        outcome.forward,
        outcome.parses,
        outcome.in_scope,
        files.join(", "),
        conjured.dropped.len(),
        conjured.goal.as_deref().unwrap_or("-")
    )
}

/// Copies the tree FROM into the existing directory TO.
fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn tiny() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/tiny")
}

#[test]
fn the_tiny_corpus_replays_and_scores() {
    let outcomes = run_corpus(&tiny(), Record::Off);
    let by_id = |id: &str| outcomes.iter().find(|o| o.id == id).unwrap().clone();
    let stub = by_id("stub-total");
    assert!(stub.forward && stub.parses && stub.in_scope);
    assert_eq!(stub.green_answered_well, None);
    let green = by_id("green-empty");
    assert!(!green.forward && green.parses && green.in_scope);
    assert_eq!(green.green_answered_well, Some(true));
}

#[test]
fn the_tiny_corpus_reaches_its_passing_scores() {
    check_corpus(&tiny(), Record::Off);
}

#[test]
#[should_panic(expected = "forward: 1 of 2, passing is 2")]
fn a_score_below_its_minimum_fails() {
    let copy = copy_of_tiny();
    set_score(copy.path(), "forward_min = 1", "forward_min = 2");
    check_corpus(copy.path(), Record::Off);
}

#[test]
#[should_panic(expected = "corpus.toml has no forward_min")]
fn a_corpus_with_no_forward_min_fails_after_its_table() {
    let copy = copy_of_tiny();
    set_score(copy.path(), "forward_min = 1\n", "");
    check_corpus(copy.path(), Record::Off);
}

#[test]
fn a_recording_asserts_nothing() {
    let copy = copy_of_tiny();
    set_score(copy.path(), "forward_min = 1", "forward_min = 2");
    check_corpus(copy.path(), Record::All);
}

fn set_score(dir: &Path, from: &str, to: &str) {
    let file = dir.join("corpus.toml");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains(from));
    std::fs::write(file, text.replace(from, to)).unwrap();
}

#[test]
#[should_panic(expected = "CONJURE_RECORD=all")]
fn a_case_with_no_reply_says_how_to_record() {
    let copy = copy_of_tiny();
    std::fs::remove_file(copy.path().join("replies/stub-total.json")).unwrap();
    run_corpus(copy.path(), Record::Off);
}

#[test]
#[should_panic(expected = "CONJURE_RECORD is refused when CI is set")]
fn record_is_refused_under_ci() {
    record_mode_from(Some("all"), true);
}

#[test]
fn record_all_is_a_mode_and_nothing_is_off() {
    assert_eq!(record_mode_from(Some("all"), false), Record::All);
    assert_eq!(record_mode_from(None, true), Record::Off);
}

#[test]
fn a_change_outside_scope_is_not_in_scope() {
    let copy = copy_of_tiny();
    let reply = r#"{"model": "haiku", "cost_usd": 0.001, "seconds": 1.0,
        "text": "<goal>fill</goal>\n<file path=\"src/shop/cart.py\">\nclass Cart:\n    def total(self):\n        return 0\n\n    def empty(self):\n        return True\n</file>\n<file path=\"src/shop/other.py\">\nx = 1\n</file>\n"}"#;
    std::fs::write(copy.path().join("replies/stub-total.json"), reply).unwrap();
    let outcomes = run_corpus(copy.path(), Record::Off);
    let stub = outcomes.iter().find(|o| o.id == "stub-total").unwrap();
    assert!(!stub.in_scope);
    assert!(stub.parses);
}

#[test]
fn also_in_scope_names_a_file_the_change_may_touch() {
    let copy = copy_of_tiny();
    let case = copy.path().join("cases/stub-total.toml");
    let mut text = std::fs::read_to_string(&case).unwrap();
    text.push_str("also_in_scope = [\"src/shop/cart__test.py\", \"src/shop/other.py\"]\n");
    std::fs::write(&case, text).unwrap();
    let outcomes = run_corpus(copy.path(), Record::Off);
    assert!(outcomes.iter().all(|o| o.in_scope));
}

#[test]
fn a_green_case_rewritten_is_not_answered_well() {
    let copy = copy_of_tiny();
    let reply = r#"{"model": "haiku", "cost_usd": 0.001, "seconds": 1.0,
        "text": "<goal>tidy</goal>\n<file path=\"src/shop/cart.py\">\nclass Cart:\n    def total(self):\n        return 0\n\n    def empty(self):\n        return not []\n</file>\n"}"#;
    std::fs::write(copy.path().join("replies/green-empty.json"), reply).unwrap();
    let outcomes = run_corpus(copy.path(), Record::Off);
    let green = outcomes.iter().find(|o| o.id == "green-empty").unwrap();
    assert_eq!(green.green_answered_well, Some(false));
}

/// A copy of the tiny corpus a test may change.
fn copy_of_tiny() -> TempDir {
    let d = TempDir::new().unwrap();
    copy_tree(&tiny(), d.path());
    d
}

/// The corpus at `corpus/conjure/<name>/`.
fn corpus_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/conjure")
        .join(name)
}

#[test]
fn bistro_reaches_its_passing_scores() {
    check_corpus(&corpus_dir("bistro"), record_mode());
}

#[test]
fn bistro_rs_reaches_its_passing_scores() {
    check_corpus(&corpus_dir("bistro-rs"), record_mode());
}
