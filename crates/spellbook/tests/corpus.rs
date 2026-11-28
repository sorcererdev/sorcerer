//! The corpora under `corpus/spellbook/<name>/`: each a project (a base copied from another
//! corpus) with the generators and rules a person would keep in `.sorcerer/spellbook/`, and cases
//! that run the engine on it and compare what it wrote, found or said with what a person would
//! accept. No model, no network. `SPELLBOOK_BLESS=1` writes the `expected/` files and prints the
//! holes of every `new` case, for a person to read before committing them.

mod support;

use std::path::{Path, PathBuf};

use support::*;

fn corpus_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/spellbook")
        .join(name)
}

#[test]
fn bistro_holds_its_cases() {
    run_new_cases(&corpus_dir("bistro"));
}

#[test]
fn bistro_rs_holds_its_cases() {
    run_new_cases(&corpus_dir("bistro-rs"));
}

#[test]
fn bistro_holds_its_fix_cases() {
    run_fix_cases(&corpus_dir("bistro"));
}

#[test]
fn bistro_rs_holds_its_fix_cases() {
    run_fix_cases(&corpus_dir("bistro-rs"));
}

#[test]
fn bistro_load_mistakes_say_what_is_wrong() {
    run_load_cases(&corpus_dir("bistro"));
}

#[test]
fn bistro_rs_load_mistakes_say_what_is_wrong() {
    run_load_cases(&corpus_dir("bistro-rs"));
}

#[test]
fn bistro_check_cases_find_what_was_planted() {
    run_check_cases(&corpus_dir("bistro"));
}

#[test]
fn bistro_rs_check_cases_find_what_was_planted() {
    run_check_cases(&corpus_dir("bistro-rs"));
}

#[test]
fn bistro_keeps_its_regions() {
    run_sync_cases(&corpus_dir("bistro"));
}

#[test]
fn bistro_rs_keeps_its_regions() {
    run_sync_cases(&corpus_dir("bistro-rs"));
}

#[test]
fn bistro_adopts_check_on_old_code() {
    run_adopt_cases(&corpus_dir("bistro"));
}

#[test]
fn bistro_rs_adopts_check_on_old_code() {
    run_adopt_cases(&corpus_dir("bistro-rs"));
}

#[test]
fn starter_holds_its_cases() {
    run_new_cases(&corpus_dir("starter"));
}
