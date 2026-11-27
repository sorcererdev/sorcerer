//! The triage eval: every case of `corpus/lackey/triage` cut by the real grammars and
//! triaged through `triage_change`, with Jev's answers replayed from the corpus's `jev/`
//! recordings, then scored against `corpus.toml`'s passing scores: how many asks are offered,
//! how many ordinary edits are.
//!
//! `LACKEY_RECORD=jev` (with `JEV_API_KEY`) asks the live Jev the questions that have no
//! recording and writes them under `jev/`: a paid call, a fraction of a cent for the corpus,
//! run by hand and refused when `CI` is set; it also deletes the recordings no case asks any
//! more. `LACKEY_EVAL_VERBOSE=1` prints every case.
mod support;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lackey::adapters::grammar_syntax::GrammarSyntax;
use lackey::domain::config::LackeyConfig;
use lackey::domain::region_record::{Label, RegionRecord};
use lackey::domain::triage_context::{FileDiff, SessionDiffs, added_file_diff, latest_change};
use lackey::ports::region_store::RegionStore;
use lackey::ports::session_changes::SessionChanges;
use lackey::usecases::triage_regions::{Look, TriageDeps, triage_change};
use serde::Deserialize;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::SystemEnv;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;
use sorcerer_models::adapters::jev::{Jev, question_key};
use sorcerer_models::adapters::recorded_decider::RecordedDecider;
use sorcerer_models::domain::decide::{Question, Weighed};
use sorcerer_models::ports::decider::Decider;
use support::{FixedClock, TempProject, open_with_vendored_grammars, test_cache};

#[derive(Deserialize)]
struct CasesFile {
    case: Vec<Case>,
}

#[derive(Deserialize, Clone)]
struct Case {
    id: String,
    scenario: String,
    file: String,
    #[serde(default)]
    prior: Vec<(String, String)>,
    edit: Vec<(String, String)>,
    region: String,
    label: String,
    #[serde(default)]
    hard: bool,
    #[serde(default)]
    held_out: bool,
}

#[derive(Deserialize)]
struct CorpusFile {
    passing_scores: PassingScores,
}

#[derive(Deserialize)]
struct PassingScores {
    /// The fewest asks (of every case not labelled `none`) that must be offered.
    asks_offered_min: usize,
    /// The most ordinary edits (cases labelled `none`) that may be offered.
    false_offers_max: usize,
}

/// What triage made of one case.
#[derive(Clone)]
struct Outcome {
    case: Case,
    /// The kind triage acts on, `-` for nothing; `None` when the cut made no region with the
    /// case's text.
    acted: Option<String>,
    regions_cut: Vec<String>,
}

#[test]
fn the_triage_corpus_reaches_its_passing_scores() {
    let corpus = corpus_dir();
    let cases: CasesFile = read_toml(&corpus.join("cases.toml"));
    let scores: CorpusFile = read_toml(&corpus.join("corpus.toml"));
    let asked = Arc::new(Mutex::new(BTreeSet::new()));
    let temp = TempProject::with(&[]);
    let project = open_with_vendored_grammars(&temp.root).unwrap();
    let syntax = Arc::new(GrammarSyntax {
        languages: project.languages.clone(),
        cache: test_cache(),
    });
    let outcomes: Vec<Outcome> = cases
        .case
        .iter()
        .map(|case| triaged(&corpus, case, &project, &syntax, &asked))
        .collect();
    if recording() {
        drop_recordings_not_asked(&corpus.join("jev"), &asked.lock().unwrap());
    }
    print_table(&outcomes);
    let uncut: Vec<&str> = outcomes
        .iter()
        .filter(|outcome| outcome.acted.is_none())
        .map(|outcome| outcome.case.id.as_str())
        .collect();
    assert!(
        uncut.is_empty(),
        "cases whose region was not cut: {uncut:?}"
    );
    let (offered, false_offers) = scored(&outcomes);
    assert!(
        offered >= scores.passing_scores.asks_offered_min,
        "{offered} asks offered, fewer than {}",
        scores.passing_scores.asks_offered_min
    );
    assert!(
        false_offers <= scores.passing_scores.false_offers_max,
        "{false_offers} false offers, more than {}",
        scores.passing_scores.false_offers_max
    );
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/lackey/triage")
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// One case: the file before and after, cut with the real grammars, triaged with the replayed
/// Jev and the scenario's session.
fn triaged(
    corpus: &Path,
    case: &Case,
    project: &Project,
    syntax: &Arc<GrammarSyntax>,
    asked: &Arc<Mutex<BTreeSet<String>>>,
) -> Outcome {
    let scenario = corpus.join("scenarios").join(&case.scenario);
    let start = std::fs::read_to_string(scenario.join("files").join(&case.file)).unwrap();
    let before = edited(&start, &case.prior, &case.id);
    let after = edited(&before, &case.edit, &case.id);
    let regions = Arc::new(MemoryRegions::default());
    let deps = TriageDeps {
        syntax: syntax.clone(),
        regions: regions.clone(),
        decider: Some(Arc::new(Noting {
            inner: recorded_decider(corpus, project),
            asked: asked.clone(),
        })),
        session: Some(Arc::new(ScenarioSession::new(
            &scenario, case, &start, &before,
        ))),
        ledger: Arc::new(JsonlLedger::for_project(project).unwrap()),
        clock: Arc::new(FixedClock::thanksgiving()),
        config: LackeyConfig::default(),
    };
    let look = Look {
        file: &case.file,
        before: &before,
        after: &after,
        landed_by_tools: &[],
    };
    triage_change(&look, &deps, &Cancel::default());
    let kept = regions.of_file(&case.file);
    let wanted = squashed(&case.region);
    Outcome {
        case: case.clone(),
        acted: kept
            .iter()
            .find(|record| squashed(&record.region.text) == wanted)
            .map(|record| {
                record
                    .triaged
                    .kind
                    .map_or("-".into(), |kind| kind.word().into())
            }),
        regions_cut: kept
            .iter()
            .map(|record| record.region.text.clone())
            .collect(),
    }
}

/// The text with each `[find, replace]` applied to the first `find`.
fn edited(text: &str, edits: &[(String, String)], id: &str) -> String {
    edits
        .iter()
        .fold(text.to_string(), |text, (find, replace)| {
            assert!(
                text.contains(find.as_str()),
                "{id}: no {find:?} in the file"
            );
            text.replacen(find.as_str(), replace, 1)
        })
}

fn squashed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn recording() -> bool {
    std::env::var("LACKEY_RECORD").is_ok()
}

/// A decider that notes the key of every question it is asked.
struct Noting {
    inner: RecordedDecider,
    asked: Arc<Mutex<BTreeSet<String>>>,
}

impl Decider for Noting {
    fn ask(
        &self,
        questions: &[Question],
        cancel: &Cancel,
    ) -> sorcerer_models::Result<Vec<Weighed>> {
        self.asked
            .lock()
            .unwrap()
            .extend(questions.iter().map(question_key));
        self.inner.ask(questions, cancel)
    }
}

fn drop_recordings_not_asked(dir: &Path, asked: &BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let key = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        if !asked.contains(key) {
            std::fs::remove_file(&path).unwrap();
        }
    }
}

/// The corpus's recordings; under `LACKEY_RECORD=jev`, the live Jev answers what is not
/// recorded.
fn recorded_decider(corpus: &Path, project: &Project) -> RecordedDecider {
    let recorded = RecordedDecider::new(corpus.join("jev"));
    match std::env::var("LACKEY_RECORD").ok().as_deref() {
        None => recorded,
        Some("jev") => {
            assert!(
                std::env::var_os("CI").is_none(),
                "LACKEY_RECORD is refused when CI is set"
            );
            let jev = Jev::from_env(&project.config.decide, &SystemEnv, None).unwrap();
            recorded.recording_with(jev)
        }
        Some(other) => panic!("LACKEY_RECORD={other}: expected jev"),
    }
}

/// The scenario's session as git would give it: the scenario's patch, file by file, with the
/// case's file (new in the patch, or changed only by the case's earlier edits) as it stood at
/// the last look, first.
struct ScenarioSession {
    diffs: SessionDiffs,
}

impl ScenarioSession {
    fn new(scenario: &Path, case: &Case, start: &str, before: &str) -> ScenarioSession {
        let patch = std::fs::read_to_string(scenario.join("session.patch")).unwrap();
        let files: Vec<FileDiff> = patch
            .split("diff --git ")
            .filter(|part| !part.trim().is_empty())
            .map(|part| {
                let header = part.lines().next().unwrap_or_default();
                let path = header.rsplit(" b/").next().unwrap_or_default().to_string();
                FileDiff {
                    path,
                    diff: format!("diff --git {part}"),
                }
            })
            .collect();
        let new_in_patch = files
            .iter()
            .any(|file| file.path == case.file && file.diff.contains("new file mode"));
        let own = if new_in_patch {
            added_file_diff(&case.file, before)
        } else {
            latest_change(&case.file, start, before)
        };
        let own = (!own.is_empty()).then(|| FileDiff {
            path: case.file.clone(),
            diff: own,
        });
        let others = files.into_iter().filter(|file| file.path != case.file);
        ScenarioSession {
            diffs: SessionDiffs {
                files: own.into_iter().chain(others).collect(),
                left_out: 0,
            },
        }
    }
}

impl SessionChanges for ScenarioSession {
    fn diffs(&self, _: &str) -> SessionDiffs {
        self.diffs.clone()
    }
}

/// Regions kept in memory, one store a case.
#[derive(Default)]
struct MemoryRegions {
    records: Mutex<HashMap<String, RegionRecord>>,
}

impl RegionStore for MemoryRegions {
    fn get(&self, id: &str) -> Option<RegionRecord> {
        self.records.lock().unwrap().get(id).cloned()
    }

    fn of_file(&self, file: &str) -> Vec<RegionRecord> {
        let records = self.records.lock().unwrap();
        let mut found: Vec<RegionRecord> = records
            .values()
            .filter(|record| record.region.file == file)
            .cloned()
            .collect();
        found.sort_by_key(|record| record.region.lines.start);
        found
    }

    fn put(&self, records: &[RegionRecord]) -> sorcerer_core::Result<()> {
        let mut kept = self.records.lock().unwrap();
        for record in records {
            kept.insert(record.region.id.clone(), record.clone());
        }
        Ok(())
    }

    fn label(&self, _: &Label) -> sorcerer_core::Result<()> {
        Ok(())
    }
}

/// Asks offered and false offers, over the cases whose region was cut.
fn scored(outcomes: &[Outcome]) -> (usize, usize) {
    let offered = |outcome: &&Outcome| outcome.acted.as_deref().is_some_and(|kind| kind != "-");
    let asks = outcomes
        .iter()
        .filter(|outcome| outcome.case.label != "none");
    let ordinary = outcomes
        .iter()
        .filter(|outcome| outcome.case.label == "none");
    (
        asks.filter(offered).count(),
        ordinary.filter(offered).count(),
    )
}

fn print_table(outcomes: &[Outcome]) {
    let verbose = std::env::var_os("LACKEY_EVAL_VERBOSE").is_some();
    let (offered, false_offers) = scored(outcomes);
    let asks = outcomes.iter().filter(|o| o.case.label != "none").count();
    let held = |o: &&Outcome| o.case.held_out;
    let held_asks = outcomes
        .iter()
        .filter(held)
        .filter(|o| o.case.label != "none")
        .count();
    let held_offered = scored(&outcomes.iter().filter(held).cloned().collect::<Vec<_>>());
    println!("# triage corpus: {offered}/{asks} asks offered, {false_offers} false offers");
    println!(
        "  held out: {}/{held_asks} asks offered, {} false offers",
        held_offered.0, held_offered.1
    );
    for outcome in outcomes {
        let acted = outcome.acted.as_deref().unwrap_or("(not cut)");
        let wrong = match outcome.case.label.as_str() {
            "none" => acted != "-",
            "ask" => acted == "-" || acted == "(not cut)",
            label => acted != label,
        };
        if verbose || wrong {
            let hard = if outcome.case.hard { " (hard)" } else { "" };
            println!(
                "  {:5} {:11} -> {acted}{hard}",
                outcome.case.id, outcome.case.label
            );
            if outcome.acted.is_none() {
                println!("        cut: {:?}", outcome.regions_cut);
            }
        }
    }
}
