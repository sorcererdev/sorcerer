//! The corpora under `corpus/spellbook/<name>/`: each a project (a base copied from another
//! corpus) with the generators and rules a person would keep in `.sorcerer/spellbook/`, and cases
//! that run the engine on it and compare what it wrote, found or said with what a person would
//! accept. No model, no network. `SPELLBOOK_BLESS=1` writes the `expected/` files and prints the
//! holes of every `new` case, for a person to read before committing them.

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use spellbook::adapters::generator_dir::generator_named;
use spellbook::domain::plan::plan_stop_text;
use spellbook::usecases::open_holes::{HolesDeps, open_holes};
use support::*;
use tempfile::TempDir;

#[derive(Deserialize)]
struct CorpusFile {
    name: String,
    language: String,
    /// The base project, relative to the corpus folder.
    base: String,
}

/// A `new` or `holes` case: a generator run with inputs, then (for `holes`) a patch applied.
#[derive(Deserialize)]
struct NewCase {
    generator: String,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
    /// A patch file next to the case, applied after the generation (the person fills a hole).
    patch: Option<String>,
    /// The holes still open at the end, one entry a place.
    #[serde(default)]
    holes: Vec<ExpectedHole>,
}

#[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ExpectedHole {
    name: String,
    kind: String,
    file: String,
    line: u32,
}

/// A `load` case: a folder of overlay files and the sentence a newcomer reads.
#[derive(Deserialize)]
struct LoadCase {
    generator: String,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
}

fn corpus_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/spellbook")
        .join(name)
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn blessing() -> bool {
    std::env::var_os("SPELLBOOK_BLESS").is_some()
}

fn unix_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// The corpus's base with its `spellbook/` folder in place as `.sorcerer/spellbook/`.
fn project_of(dir: &Path, corpus: &CorpusFile) -> TempDir {
    let project = tempfile::tempdir().unwrap();
    copy_tree(&dir.join(&corpus.base), project.path());
    let target = project.path().join(".sorcerer/spellbook");
    std::fs::create_dir_all(&target).unwrap();
    copy_tree(&dir.join("spellbook"), &target);
    project
}

fn case_ids(dir: &Path, kind: &str, extension: &str) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(dir.join("cases").join(kind))
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|e| e == extension))
        .filter_map(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
        .collect();
    ids.sort();
    ids
}

fn git_apply(root: &Path, patch: &Path) {
    let status = std::process::Command::new("git")
        .args(["apply", "--whitespace=nowarn"])
        .arg(patch)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git apply {}", patch.display());
}

/// The holes still open in a project, one entry a place.
fn open_places(rig: &PlanRig) -> Vec<ExpectedHole> {
    let store = rig.apply_rig().holes;
    let deps = HolesDeps {
        view: &rig.view,
        holes: &store,
    };
    let mut places: Vec<ExpectedHole> = open_holes(&deps, None)
        .unwrap()
        .into_iter()
        .map(|hole| ExpectedHole {
            name: hole.name,
            kind: hole.kind,
            file: hole.file,
            line: hole.line,
        })
        .collect();
    places.sort();
    places
}

/// Runs one `new` (or `holes`) case: the files it wrote against `expected/new/<id>/`, the holes
/// against the case's list.
fn run_new_case(dir: &Path, corpus: &CorpusFile, kind: &str, id: &str) {
    let case: NewCase = read_toml(&dir.join("cases").join(kind).join(format!("{id}.toml")));
    let project = project_of(dir, corpus);
    let rig = PlanRig::new(project.path());
    assert_eq!(rig.generator(&case.generator).language, corpus.language);
    let given: Vec<(&str, &str)> = case
        .inputs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let plan = rig
        .plan(&case.generator, &given)
        .unwrap_or_else(|stop| panic!("{kind}/{id}: {}", plan_stop_text(&stop)));
    let written = rig.apply(plan).written;
    assert!(!written.is_empty(), "{kind}/{id} wrote nothing");
    if let Some(patch) = &case.patch {
        git_apply(project.path(), &dir.join("cases").join(kind).join(patch));
    }
    let expected_dir = dir.join("expected").join(kind).join(id);
    for path in &written {
        let got = std::fs::read_to_string(project.path().join(path)).unwrap();
        let expected_path = expected_dir.join(path);
        if blessing() {
            std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
            std::fs::write(&expected_path, &got).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("{}: {e}", expected_path.display()));
        assert_eq!(
            unix_newlines(&got),
            unix_newlines(&expected),
            "{kind}/{id}: {path}"
        );
    }
    let holes = open_places(&rig);
    if blessing() {
        println!("# {kind}/{id}");
        for hole in &holes {
            println!(
                "[[holes]]\nname = {:?}\nkind = {:?}\nfile = {:?}\nline = {}\n",
                hole.name, hole.kind, hole.file, hole.line
            );
        }
        return;
    }
    let mut want = case.holes;
    want.sort();
    assert_eq!(holes, want, "{kind}/{id}: the open holes");
}

fn run_new_cases(name: &str) {
    let dir = corpus_dir(name);
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    assert_eq!(corpus.name, name);
    for kind in ["new", "holes"] {
        for id in case_ids(&dir, kind, "toml") {
            run_new_case(&dir, &corpus, kind, &id);
        }
    }
}

#[test]
fn bistro_holds_its_cases() {
    run_new_cases("bistro");
}

#[test]
fn bistro_rs_holds_its_cases() {
    run_new_cases("bistro-rs");
}

/// What a newcomer reads for a load case: the generator's problem, or the plan's stop.
fn load_sentence(dir: &Path, corpus: &CorpusFile, id: &str) -> String {
    let case_dir = dir.join("cases/load").join(id);
    let case: LoadCase = read_toml(&case_dir.join("case.toml"));
    let project = project_of(dir, corpus);
    let overlay = case_dir.join("spellbook");
    copy_tree(&overlay, &project.path().join(".sorcerer/spellbook"));
    let rig = PlanRig::new(project.path());
    let loaded = generator_named(&rig.project, &case.generator)
        .unwrap_or_else(|| panic!("load/{id}: no generator {}", case.generator));
    if let Err(problem) = loaded {
        return problem.to_string();
    }
    let given: Vec<(&str, &str)> = case
        .inputs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    match rig.plan(&case.generator, &given) {
        Ok(_) => panic!("load/{id}: the generator loaded and planned; the mistake is gone"),
        Err(stop) => plan_stop_text(&stop),
    }
}

fn run_load_cases(name: &str) {
    let dir = corpus_dir(name);
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    let cases = dir.join("cases/load");
    let mut ids: Vec<String> = std::fs::read_dir(&cases)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    ids.sort();
    for id in ids {
        let got = load_sentence(&dir, &corpus, &id);
        let path = cases.join(&id).join("expected.txt");
        if blessing() {
            std::fs::write(&path, format!("{got}\n")).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            unix_newlines(&got),
            unix_newlines(expected.trim_end()),
            "load/{id}"
        );
    }
}

#[test]
fn bistro_load_mistakes_say_what_is_wrong() {
    run_load_cases("bistro");
}
