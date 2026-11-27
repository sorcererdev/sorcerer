//! Every shipped language against its corpus, `corpus/languages/<name>/`: its grammar is on
//! disk for the tests, every query compiles against it, every corpus file of the language
//! parses, and its units match `expected/units.txt`. A language whose queries capture no unit
//! (a config or markup format) has no `units.txt`. Each test checks every language and lists
//! every problem. No network: run `scripts/test-grammars` first.

mod support;

use std::path::Path;

use sorcerer_core::domain::facts::{facts_of, parse_errors_of};
use sorcerer_core::domain::language::{Languages, Loaded};
use sorcerer_core::domain::unit::{UnitKind, units_of};
use std::sync::Arc;
use support::*;

/// The shipped languages, each grammar on its test sources.
fn shipped() -> Languages {
    let nowhere = tempfile::TempDir::new().unwrap();
    let mut languages = Languages::assemble(nowhere.path(), &[]).unwrap();
    point_at_test_grammars(&mut languages);
    languages
}

/// The language loaded, or the problem a builder reads.
fn loaded(languages: &Languages, name: &str) -> Result<Arc<Loaded>, String> {
    let language = languages.get(name).ok_or("not a shipped language")?;
    if !has_test_grammar(language) {
        return Err(format!(
            "{name}: its grammar is not on disk at {}: run scripts/test-grammars",
            test_grammar_sources(language).display()
        ));
    }
    languages
        .load(name, &test_cache())
        .map_err(|e| format!("{name}: {e}"))
}

/// The base files a language covers.
fn files_of(languages: &Languages, name: &str) -> Vec<(String, String)> {
    base_files(name)
        .into_iter()
        .filter(|(path, _)| {
            languages
                .for_path(Path::new(path))
                .is_some_and(|language| language.name == name)
        })
        .collect()
}

#[test]
fn every_shipped_language_has_a_corpus_with_files_of_its_own() {
    let languages = shipped();
    let problems: Vec<String> = shipped_language_names()
        .iter()
        .filter_map(|name| {
            let corpus = language_corpus(name);
            if !corpus.join("corpus.toml").is_file() {
                return Some(format!(
                    "{name}: no {}",
                    corpus.join("corpus.toml").display()
                ));
            }
            files_of(&languages, name)
                .is_empty()
                .then(|| format!("{name}: base/ holds no file its `files` globs match"))
        })
        .collect();
    assert_no_problems("language corpora", &problems);
}

#[test]
fn every_query_compiles_against_its_grammar() {
    let languages = shipped();
    let problems: Vec<String> = shipped_language_names()
        .iter()
        .filter_map(|name| loaded(&languages, name).err())
        .collect();
    assert_no_problems("grammars and queries", &problems);
}

#[test]
fn every_corpus_file_parses_without_an_error() {
    let languages = shipped();
    let mut problems = Vec::new();
    for name in shipped_language_names() {
        let Ok(language) = loaded(&languages, &name) else {
            continue;
        };
        for (path, text) in files_of(&languages, &name) {
            match parse_errors_of(&language, &text) {
                Ok(lines) if lines.is_empty() => {}
                Ok(lines) => {
                    problems.push(format!("{name}: {path} does not parse at lines {lines:?}"))
                }
                Err(e) => problems.push(format!("{name}: {path}: {e}")),
            }
        }
    }
    assert_no_problems("corpus parses", &problems);
}

/// One line per unit of each file: `booking/models.py:4-9 class Booking`, with ` stub` when
/// its body does nothing yet.
fn units_text(language: &Loaded, files: &[(String, String)]) -> String {
    let mut text = String::new();
    for (path, source) in files {
        for unit in units_of(language, source).unwrap_or_default() {
            let kind = kind_word(&unit.kind);
            let stub = facts_of(language, source, &unit).is_ok_and(|facts| facts.body_empty);
            text.push_str(&format!(
                "{path}:{}-{} {kind} {}{}\n",
                unit.span.start,
                unit.span.end,
                unit.name,
                if stub { " stub" } else { "" }
            ));
        }
    }
    text
}

/// `function`, `method`, `class`, `impl`, `type`, or the kind a language names (`module`).
fn kind_word(kind: &UnitKind) -> String {
    match kind {
        UnitKind::Other(word) => word.clone(),
        known => format!("{known:?}").to_lowercase(),
    }
}

#[test]
fn the_units_of_every_corpus_match_its_golden() {
    let languages = shipped();
    let mut problems = Vec::new();
    for name in shipped_language_names() {
        let Ok(language) = loaded(&languages, &name) else {
            continue;
        };
        let golden = language_corpus(&name).join("expected/units.txt");
        if !language.language.has_unit_captures() {
            if golden.exists() {
                problems.push(format!(
                    "{name}: its queries capture no unit, so it has no {}",
                    golden.display()
                ));
            }
            continue;
        }
        let got = units_text(&language, &files_of(&languages, &name));
        if got.is_empty() {
            problems.push(format!("{name}: a language with units found none in base/"));
        }
        if let Err(problem) = matches_golden(&golden, &got) {
            problems.push(format!("{name}: {problem}"));
        }
    }
    assert_no_problems("units", &problems);
}
