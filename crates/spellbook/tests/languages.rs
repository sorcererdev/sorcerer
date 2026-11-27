//! Every shipped language's spellbook cases, in `corpus/languages/<name>/` (the same case kinds
//! as `corpus.rs`, run by the same runners): its `check` cases (a `clean` one at least), its
//! `new` cases when the language has `[holes]` (between them, and the `fix` cases, a hole of
//! every kind the language has, each written where it parses), its `sync` cases when the
//! language has comments to mark a region with, and its `fix` cases. Each test checks every
//! language and lists every one that fails. `LANGUAGES_BLESS=1` writes the `expected/` files.

mod support;

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use serde::Deserialize;
use sorcerer_core::domain::language::{Language, Languages};
use spellbook::domain::holes_table::holes_table;
use support::*;

fn shipped_language(name: &str) -> Language {
    let nowhere = tempfile::TempDir::new().unwrap();
    Languages::assemble(nowhere.path(), &[])
        .unwrap()
        .get(name)
        .unwrap_or_else(|| panic!("{name} is not a shipped language"))
        .clone()
}

/// Runs `run` for every shipped language whose corpus it applies to, and lists the languages it
/// panicked for, with the panic's words.
fn for_every_language(what: &str, run: impl Fn(&str, &Path)) {
    let mut problems = Vec::new();
    for name in shipped_language_names() {
        let dir = language_corpus(&name);
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| run(&name, &dir))) {
            let words = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            problems.push(format!("{name}: {words}"));
        }
    }
    assert_no_problems(what, &problems);
}

fn has_cases(dir: &Path, kind: &str) -> bool {
    std::fs::read_dir(dir.join("cases").join(kind))
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.path().extension().is_some_and(|e| e == "toml"))
}

#[test]
fn every_language_holds_its_check_cases() {
    for_every_language("check cases", |_, dir| run_check_cases(dir));
}

#[test]
fn every_language_with_holes_holds_its_new_cases() {
    for_every_language("new cases", |name, dir| {
        if holes_table(&shipped_language(name))
            .unwrap()
            .kinds
            .is_empty()
        {
            return;
        }
        assert!(has_cases(dir, "new"), "it has [holes] and no cases/new/");
        run_new_cases(dir);
    });
}

#[test]
fn every_language_with_comments_keeps_its_regions() {
    for_every_language("sync cases", |name, dir| {
        if shipped_language(name).comment_marks().is_none() {
            return;
        }
        run_sync_cases(dir);
    });
}

#[test]
fn every_language_holds_its_fix_cases() {
    for_every_language("fix cases", |_, dir| {
        if has_cases(dir, "fix") {
            run_fix_cases(dir);
        }
    });
}

/// The `[[holes]]` a case expects, by kind.
#[derive(Deserialize)]
struct CaseHoles {
    #[serde(default)]
    holes: Vec<HoleKind>,
}

#[derive(Deserialize)]
struct HoleKind {
    kind: String,
}

/// The kinds of the holes the language's `new` and `fix` cases leave open.
fn kinds_the_cases_leave(dir: &Path) -> BTreeSet<String> {
    ["new", "fix"]
        .iter()
        .flat_map(|kind| {
            std::fs::read_dir(dir.join("cases").join(kind))
                .into_iter()
                .flatten()
                .flatten()
        })
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .flat_map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            let case: CaseHoles =
                toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            case.holes.into_iter().map(|hole| hole.kind)
        })
        .collect()
}

#[test]
fn every_hole_kind_of_every_language_is_written_by_a_case() {
    for_every_language("hole kinds", |name, dir| {
        let kinds: BTreeSet<String> = holes_table(&shipped_language(name))
            .unwrap()
            .kinds
            .into_keys()
            .collect();
        let left = kinds_the_cases_leave(dir);
        let missing: Vec<&String> = kinds.difference(&left).collect();
        assert!(
            missing.is_empty(),
            "no new or fix case leaves a hole of kind {missing:?} ([holes] has {kinds:?})"
        );
    });
}
