//! Every shipped language with units, in `corpus/languages/<name>/`: conjure asked in the file
//! its `corpus.toml` names (`[conjure] file`) finds the related files and the test file in
//! `expected/related.txt`. A language without units has no `[conjure]` (conjure never runs in
//! it). No model, no network. `LANGUAGES_BLESS=1` writes the goldens.

mod support;

use std::path::Path;

use conjure::adapters::imports::related_files;
use conjure::domain::language_tables::tests_path;
use serde::Deserialize;
use sorcerer_core::domain::language::Languages;
use sorcerer_core::ports::view::CodebaseView;
use support::*;

#[derive(Deserialize)]
struct CorpusFile {
    conjure: Option<ConjureCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConjureCase {
    /// The file conjure is asked in, relative to the base.
    file: String,
}

/// What conjure finds for the file: `tests <path>` (or `tests none`), then one line per related
/// file, `<closeness> <path>`.
fn related_text(dir: &Path, name: &str, file: &str) -> Result<String, String> {
    let project_dir = git_project();
    copy_folder(&dir.join("base"), project_dir.path());
    commit(project_dir.path(), "base");
    let (project, view) = project_with_vendored_grammars(project_dir.path());
    let path = Path::new(file);
    let language = project
        .languages
        .for_path(path)
        .filter(|language| language.name == name)
        .ok_or_else(|| format!("{file} is not a {name} file"))?;
    let tests = tests_path(language, path)
        .map_err(|e| e.to_string())?
        .map_or("none".to_string(), |p| {
            p.to_string_lossy().replace('\\', "/")
        });
    let loaded = project
        .languages
        .load(name, &test_cache())
        .map_err(|e| e.to_string())?;
    let text = view.text(path).map_err(|e| e.to_string())?;
    let related =
        related_files(&view, &loaded, path, &text, &|_| true).map_err(|e| e.to_string())?;
    let lines: String = related
        .files
        .iter()
        .map(|file| {
            format!(
                "{:?} {}\n",
                file.closeness,
                file.path.to_string_lossy().replace('\\', "/")
            )
        })
        .collect();
    Ok(format!("tests {tests}\n{lines}"))
}

#[test]
fn every_language_with_units_relates_its_files_and_finds_its_tests() {
    let mut problems = Vec::new();
    for name in shipped_language_names() {
        let dir = language_corpus(&name);
        let corpus: CorpusFile = match std::fs::read_to_string(dir.join("corpus.toml"))
            .map_err(|e| e.to_string())
            .and_then(|text| toml::from_str(&text).map_err(|e| e.to_string()))
        {
            Ok(corpus) => corpus,
            Err(e) => {
                problems.push(format!("{name}: corpus.toml: {e}"));
                continue;
            }
        };
        let nowhere = tempfile::tempdir().unwrap();
        let has_units = Languages::assemble(nowhere.path(), &[])
            .unwrap()
            .get(&name)
            .is_some_and(|language| language.has_unit_captures());
        let outcome = match (corpus.conjure, has_units) {
            (None, false) => continue,
            (Some(_), false) => Err("a language without units has no [conjure]".to_string()),
            (None, true) => Err("a language with units has [conjure] file = \"…\"".to_string()),
            (Some(case), true) => related_text(&dir, &name, &case.file)
                .and_then(|got| matches_golden(&dir.join("expected/related.txt"), &got)),
        };
        if let Err(problem) = outcome {
            problems.push(format!("{name}: {problem}"));
        }
    }
    assert_no_problems("conjure related files", &problems);
}
