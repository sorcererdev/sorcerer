//! The case runners of spellbook's corpora: a corpus folder holds `corpus.toml`, a base, its
//! `spellbook/` and `cases/<kind>/` with `expected/<kind>/`. `corpus.rs` runs them over
//! `corpus/spellbook/<name>/`, and `languages.rs` over `corpus/languages/<name>/`. No model, no
//! network. `SPELLBOOK_BLESS=1` (or `LANGUAGES_BLESS=1`) writes the `expected/` files and
//! prints the holes of every `new` case, for a person to read before committing them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use serde::Deserialize;
use spellbook::adapters::generator_dir::generator_named;
use spellbook::domain::plan::plan_stop_text;
use spellbook::usecases::check_files::check_files;
use spellbook::usecases::check_project::check_project;
use spellbook::usecases::open_holes::{HolesDeps, open_holes};
use spellbook::usecases::sync_regions::{SyncMode, plan_sync};
use tempfile::TempDir;

#[derive(Deserialize)]
pub struct CorpusFile {
    name: String,
    language: String,
    /// The base project, relative to the corpus folder; none is an empty project (a first
    /// generation).
    base: Option<String>,
    /// A folder of files laid over the base for every case that does not say `base =
    /// "shared"`: the files that carry region markers.
    overlay: Option<String>,
}

/// Which project a case starts from: the base with the corpus's overlay (the default), or the
/// shared base as the other corpora have it (`base = "shared"`).
#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Base {
    #[default]
    Overlaid,
    Shared,
}

/// A `new` or `holes` case: a generator run with inputs, then (for `holes`) a patch applied.
#[derive(Deserialize)]
pub struct NewCase {
    generator: String,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
    /// A patch file next to the case, applied after the generation (the person fills a hole).
    patch: Option<String>,
    /// The holes still open at the end, one entry a place.
    #[serde(default)]
    holes: Vec<ExpectedHole>,
    #[serde(default)]
    base: Base,
}

#[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExpectedHole {
    name: String,
    kind: String,
    file: String,
    line: u32,
}

/// A `load` case: a folder of overlay files and the sentence a newcomer reads.
#[derive(Deserialize)]
pub struct LoadCase {
    /// The generator whose load or plan is the mistake; none for a mistake in the rules.
    generator: Option<String>,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
    /// A file to check, for a mistake that only shows when a rule runs.
    check: Option<String>,
    /// The mistake is in `regions.toml`: the sentence is what a sync says.
    #[serde(default)]
    sync: bool,
}

/// A `fix` case: a patch over the base plants a finding, then one of its fixes is planned and
/// applied with the case's inputs.
#[derive(Deserialize)]
pub struct FixCase {
    patch: String,
    /// The finding, `FILE:LINE`.
    at: String,
    #[serde(default = "first_fix")]
    fix: usize,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
    #[serde(default)]
    holes: Vec<ExpectedHole>,
    #[serde(default)]
    base: Base,
}

pub fn first_fix() -> usize {
    1
}

/// A `check` case: a patch over the base, then every file checked.
#[derive(Deserialize)]
pub struct CheckCase {
    patch: Option<String>,
    #[serde(default)]
    base: Base,
}

/// A `sync` case: a patch over the base, then a command (`spellbook sync` with the case's
/// `mode`, or the case's whole `command`); what it printed is `expected/sync/<id>.txt`, every
/// file it changed is under `expected/sync/<id>/`.
#[derive(Deserialize)]
pub struct SyncCase {
    /// `refresh`, `init NAME`, `check` or `plan`.
    #[serde(default)]
    mode: Option<String>,
    /// The words after `spellbook`, instead of a sync (a `new` on broken markers).
    command: Option<String>,
    patch: Option<String>,
    #[serde(default)]
    base: Base,
    exit: i32,
}

pub fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn blessing() -> bool {
    std::env::var_os("SPELLBOOK_BLESS").is_some() || std::env::var_os("LANGUAGES_BLESS").is_some()
}

pub fn unix_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// The corpus's base (with its overlay, unless the case starts from the shared base) and its
/// `spellbook/` folder in place as `.sorcerer/spellbook/`.
pub fn project_of(dir: &Path, corpus: &CorpusFile, base: Base) -> TempDir {
    let project = tempfile::tempdir().unwrap();
    if let Some(base) = &corpus.base {
        copy_tree(&dir.join(base), project.path());
    }
    if let (Some(overlay), Base::Overlaid) = (&corpus.overlay, base) {
        copy_tree(&dir.join(overlay), project.path());
    }
    let target = project.path().join(".sorcerer/spellbook");
    std::fs::create_dir_all(&target).unwrap();
    copy_tree(&dir.join("spellbook"), &target);
    project
}

pub fn case_ids(dir: &Path, kind: &str, extension: &str) -> Vec<String> {
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

pub fn git_apply(root: &Path, patch: &Path) {
    let status = std::process::Command::new("git")
        .args(["apply", "--whitespace=nowarn"])
        .arg(patch)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git apply {}", patch.display());
}

/// The holes still open in a project, one entry a place.
pub fn open_places(rig: &PlanRig) -> Vec<ExpectedHole> {
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
pub fn run_new_case(dir: &Path, corpus: &CorpusFile, kind: &str, id: &str) {
    let case: NewCase = read_toml(&dir.join("cases").join(kind).join(format!("{id}.toml")));
    let project = project_of(dir, corpus, case.base);
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
    let applied = rig.apply(plan);
    assert!(!applied.written.is_empty(), "{kind}/{id} wrote nothing");
    let written: Vec<String> = applied.written.into_iter().chain(applied.changed).collect();
    if let Some(patch) = &case.patch {
        git_apply(project.path(), &dir.join("cases").join(kind).join(patch));
    }
    let expected_dir = dir.join("expected").join(kind).join(id);
    for path in &written {
        let got = std::fs::read(project.path().join(path)).unwrap();
        let expected_path = expected_dir.join(path);
        if blessing() {
            std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
            std::fs::write(&expected_path, &got).unwrap();
            continue;
        }
        let expected = std::fs::read(&expected_path)
            .unwrap_or_else(|e| panic!("{}: {e}", expected_path.display()));
        assert_same_file(&got, &expected, &format!("{kind}/{id}: {path}"));
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

/// A written file against its expectation: text with any newlines, a binary file (a copied font)
/// byte for byte.
fn assert_same_file(got: &[u8], expected: &[u8], what: &str) {
    match (std::str::from_utf8(got), std::str::from_utf8(expected)) {
        (Ok(got), Ok(expected)) => {
            assert_eq!(unix_newlines(got), unix_newlines(expected), "{what}")
        }
        _ => assert!(got == expected, "{what}: the bytes differ"),
    }
}

pub fn run_new_cases(dir: &Path) {
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    for kind in ["new", "holes"] {
        for id in case_ids(dir, kind, "toml") {
            run_new_case(dir, &corpus, kind, &id);
        }
    }
}

/// What a newcomer reads for a load case: the generator's problem, or the plan's stop.
pub fn load_sentence(dir: &Path, corpus: &CorpusFile, id: &str) -> String {
    let case_dir = dir.join("cases/load").join(id);
    let case: LoadCase = read_toml(&case_dir.join("case.toml"));
    let project = project_of(dir, corpus, Base::Overlaid);
    let overlay = case_dir.join("spellbook");
    copy_tree(&overlay, &project.path().join(".sorcerer/spellbook"));
    let rig = PlanRig::new(project.path());
    if case.sync {
        return match plan_sync(&rig.deps(), &SyncMode::Refresh) {
            Ok(_) => panic!("load/{id}: the regions loaded and synced; the mistake is gone"),
            Err(stop) => plan_stop_text(&stop),
        };
    }
    let Some(generator) = &case.generator else {
        return rules_sentence(project.path(), id, case.check.as_deref());
    };
    let loaded = generator_named(&rig.project, generator)
        .unwrap_or_else(|| panic!("load/{id}: no generator {generator}"));
    if let Err(problem) = loaded {
        return problem.to_string();
    }
    let given: Vec<(&str, &str)> = case
        .inputs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    match rig.plan(generator, &given) {
        Ok(_) => panic!("load/{id}: the generator loaded and planned; the mistake is gone"),
        Err(stop) => plan_stop_text(&stop),
    }
}

/// What a newcomer reads for a mistake in the rules: the rules file's problem, or the sentence
/// of the rule that could not run on the file.
pub fn rules_sentence(root: &Path, id: &str, check: Option<&str>) -> String {
    let wired = wired_at(root);
    let checking = match wired.checking() {
        Ok(checking) => checking,
        Err(problem) => return problem,
    };
    let file = check.unwrap_or_else(|| panic!("load/{id}: the rules load; name a file to `check`"));
    match check_files(&checking.deps(), &[PathBuf::from(file)]) {
        Ok(_) => panic!("load/{id}: the rules ran; the mistake is gone"),
        Err(e) => e.to_string(),
    }
}

/// Runs one `fix` case: every file it changed or wrote against `expected/fix/<id>/`, the open
/// holes against the case's list.
pub fn run_fix_case(dir: &Path, corpus: &CorpusFile, id: &str) {
    use spellbook::domain::inputs::Given;
    use spellbook::usecases::apply_plan::{ChangesLanded, apply_plan};
    use spellbook::usecases::plan_fix::{FindingAt, plan_fix};

    let cases = dir.join("cases/fix");
    let case: FixCase = read_toml(&cases.join(format!("{id}.toml")));
    let project = project_of(dir, corpus, case.base);
    git_apply(project.path(), &cases.join(&case.patch));
    let (file, line) = case.at.rsplit_once(':').expect("at is FILE:LINE");
    let at = FindingAt {
        file: file.into(),
        line: line.parse().expect("a line number"),
        rule: None,
        index: case.fix,
    };
    let wired = wired_at(project.path());
    let checking = wired.checking().unwrap();
    let plan = plan_fix(&checking.fix_deps(), &at, &Given(case.inputs.clone()))
        .unwrap_or_else(|stop| panic!("fix/{id}: {}", plan_stop_text(&stop)));
    let applied = apply_plan(&wired.apply_deps(), plan, ChangesLanded::NotYet).unwrap();
    assert_eq!(applied.refused, None, "fix/{id}");
    let expected_dir = dir.join("expected/fix").join(id);
    for path in applied.changed.iter().chain(&applied.written) {
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
            "fix/{id}: {path}"
        );
    }
    let holes = open_places(&PlanRig::new(project.path()));
    if blessing() {
        println!("# fix/{id}");
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
    assert_eq!(holes, want, "fix/{id}: the open holes");
}

pub fn run_fix_cases(dir: &Path) {
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    let ids = case_ids(dir, "fix", "toml");
    assert!(!ids.is_empty(), "{} has no fix cases", corpus.name);
    for id in ids {
        run_fix_case(dir, &corpus, &id);
    }
}

pub fn run_load_cases(dir: &Path) {
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
    assert!(!ids.is_empty(), "{} has no load cases", corpus.name);
    for id in ids {
        let got = load_sentence(dir, &corpus, &id);
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

/// Runs one `check` case: the patch applied, every file checked, the findings against
/// `expected/check/<id>.json`.
pub fn run_check_case(dir: &Path, corpus: &CorpusFile, id: &str) {
    let case: CheckCase = read_toml(&dir.join(format!("cases/check/{id}.toml")));
    let project = project_of(dir, corpus, case.base);
    if let Some(patch) = &case.patch {
        git_apply(project.path(), &dir.join("cases/check").join(patch));
    }
    let wired = wired_at(project.path());
    let checking = wired
        .checking()
        .unwrap_or_else(|why| panic!("check/{id}: {why}"));
    let found = check_project(&checking.deps()).unwrap();
    let got = serde_json::to_string_pretty(&found).unwrap() + "\n";
    let path = dir.join(format!("expected/check/{id}.json"));
    if blessing() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let expected =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(unix_newlines(&got), unix_newlines(&expected), "check/{id}");
    if id == "clean" {
        assert!(found.is_empty(), "check/clean found {found:#?}");
    }
}

pub fn run_check_cases(dir: &Path) {
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    let ids = case_ids(dir, "check", "toml");
    assert!(
        ids.iter().any(|id| id == "clean"),
        "{} has no clean case",
        corpus.name
    );
    for id in ids {
        run_check_case(dir, &corpus, &id);
    }
}

/// Every text file of a project but spellbook's own folder, by its path with `/`.
pub fn texts_of(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, found: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if name == ".sorcerer" || name == ".git" || name == "target" {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, found);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                found.insert(name, text);
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}

/// The words a sync case runs after `spellbook`.
pub fn sync_words(case: &SyncCase) -> Vec<String> {
    if let Some(command) = &case.command {
        return command.split_whitespace().map(String::from).collect();
    }
    let mode = case.mode.as_deref().unwrap_or("refresh");
    let mut words = vec!["sync".to_string()];
    match mode.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["refresh"] => {}
        ["check"] => words.push("--check".into()),
        ["plan"] => words.push("--plan".into()),
        ["init", name] => words.extend(["--init".to_string(), name.to_string()]),
        _ => panic!("mode = {mode:?} is not refresh, init NAME, check or plan"),
    }
    words
}

/// Runs the command line in a project on the vendored grammars and a fixed clock: the exit
/// code, and what it printed and said, in that order.
pub fn run_spellbook(root: &Path, words: &[String]) -> (i32, String) {
    use sorcerer_core::adapters::env::MapEnv;
    use sorcerer_core::domain::project::Project;
    use spellbook::adapters::wiring::{Wired, wired};

    let env = MapEnv::default().with_user_dirs(root);
    let wire = |mut project: Project| -> sorcerer_core::Result<Wired> {
        point_at_vendored(&mut project.languages);
        wired(
            project,
            test_cache(),
            std::sync::Arc::new(FixedClock::at_noon()),
        )
    };
    let args: Vec<std::ffi::OsString> = std::iter::once("spellbook".to_string())
        .chain(words.iter().cloned())
        .map(Into::into)
        .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = spellbook::cli::main_with(args, root, &env, &wire, &mut out, &mut err);
    let printed = String::from_utf8(out).unwrap() + &String::from_utf8(err).unwrap();
    (code, printed)
}

/// Runs one `sync` case: the patch applied, the command run, what it printed against
/// `expected/sync/<id>.txt`, its exit code against the case's, every file it changed against
/// `expected/sync/<id>/`.
pub fn run_sync_case(dir: &Path, corpus: &CorpusFile, id: &str) {
    let cases = dir.join("cases/sync");
    let case: SyncCase = read_toml(&cases.join(format!("{id}.toml")));
    let project = project_of(dir, corpus, case.base);
    if let Some(patch) = &case.patch {
        git_apply(project.path(), &cases.join(patch));
    }
    let before = texts_of(project.path());
    let (code, printed) = run_spellbook(project.path(), &sync_words(&case));
    let after = texts_of(project.path());
    let changed: Vec<&String> = after
        .iter()
        .filter(|(path, text)| before.get(*path) != Some(*text))
        .map(|(path, _)| path)
        .collect();
    let printed_path = dir.join(format!("expected/sync/{id}.txt"));
    let expected_dir = dir.join("expected/sync").join(id);
    if blessing() {
        std::fs::create_dir_all(printed_path.parent().unwrap()).unwrap();
        std::fs::write(&printed_path, &printed).unwrap();
        for path in &changed {
            let to = expected_dir.join(path);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::write(&to, &after[*path]).unwrap();
        }
        println!("# sync/{id}: exit {code}");
        return;
    }
    let expected = std::fs::read_to_string(&printed_path)
        .unwrap_or_else(|e| panic!("{}: {e}", printed_path.display()));
    assert_eq!(
        unix_newlines(&printed),
        unix_newlines(&expected),
        "sync/{id}"
    );
    assert_eq!(code, case.exit, "sync/{id}: the exit code");
    let mut want: Vec<String> = Vec::new();
    if expected_dir.is_dir() {
        want = texts_of(&expected_dir).into_keys().collect();
    }
    let got: Vec<String> = changed.iter().map(|path| path.to_string()).collect();
    assert_eq!(got, want, "sync/{id}: the files changed");
    for path in &got {
        let expected = std::fs::read_to_string(expected_dir.join(path)).unwrap();
        assert_eq!(
            unix_newlines(&after[path]),
            unix_newlines(&expected),
            "sync/{id}: {path}"
        );
    }
}

pub fn run_sync_cases(dir: &Path) {
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    let ids = case_ids(dir, "sync", "toml");
    assert!(!ids.is_empty(), "{} has no sync cases", corpus.name);
    for id in ids {
        run_sync_case(dir, &corpus, &id);
    }
}

/// An `adopt` case: patches over the base, committed in a fresh git repository; more patches
/// over that (the branch's work); then commands after `spellbook`, in order. What they printed,
/// each after a `$ spellbook …` line, is `expected/adopt/<id>.txt`; `exit` is the last one's.
#[derive(Deserialize)]
pub struct AdoptCase {
    #[serde(default)]
    committed: Vec<String>,
    #[serde(default)]
    patches: Vec<String>,
    commands: Vec<String>,
    exit: i32,
}

pub fn git_in(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=corpus",
            "-c",
            "user.email=corpus@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

/// Runs one `adopt` case: the committed patches, the commit, the branch's patches, the commands,
/// what they printed against `expected/adopt/<id>.txt` and the last exit code against the case's.
pub fn run_adopt_case(dir: &Path, corpus: &CorpusFile, id: &str) {
    let cases = dir.join("cases/adopt");
    let case: AdoptCase = read_toml(&cases.join(format!("{id}.toml")));
    let project = project_of(dir, corpus, Base::Overlaid);
    for patch in &case.committed {
        git_apply(project.path(), &cases.join(patch));
    }
    if !case.committed.is_empty() {
        git_in(project.path(), &["init", "-q"]);
        git_in(project.path(), &["add", "-A"]);
        git_in(project.path(), &["commit", "-q", "-m", "base"]);
    }
    for patch in &case.patches {
        git_apply(project.path(), &cases.join(patch));
    }
    let mut printed = String::new();
    let mut code = 0;
    for command in &case.commands {
        let words: Vec<String> = command.split_whitespace().map(String::from).collect();
        let (exit, said) = run_spellbook(project.path(), &words);
        printed.push_str(&format!("$ spellbook {command}\n{said}"));
        code = exit;
    }
    let path = dir.join(format!("expected/adopt/{id}.txt"));
    if blessing() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &printed).unwrap();
        println!("# adopt/{id}: exit {code}");
        return;
    }
    let expected =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        unix_newlines(&printed),
        unix_newlines(&expected),
        "adopt/{id}"
    );
    assert_eq!(code, case.exit, "adopt/{id}: the exit code");
}

pub fn run_adopt_cases(dir: &Path) {
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    let ids = case_ids(dir, "adopt", "toml");
    assert!(!ids.is_empty(), "{} has no adopt cases", corpus.name);
    for id in ids {
        run_adopt_case(dir, &corpus, &id);
    }
}
