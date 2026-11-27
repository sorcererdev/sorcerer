//! Adopting `check` on code with debt: the baseline, `check --since` and `[spellbook.rules]`, on
//! the command line over a copy of bistro.

mod support;

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::domain::project::Project;
use spellbook::adapters::wiring::{Wired, wired};
use spellbook::cli::main_with;
use support::*;

const CLOSE: &str = "src/bistro/booking/commands/close_table.py";
const OPEN: &str = "src/bistro/booking/commands/open_table.py";
const SQLITE: &str = "from dataclasses import dataclass\nimport sqlite3\n";

/// Runs a command in a directory: the exit code and what it printed and said.
fn run(dir: &Path, rest: &[&str]) -> (i32, String) {
    let env = MapEnv::default().with_user_dirs(dir);
    let wire = |mut project: Project| -> sorcerer_core::Result<Wired> {
        point_at_vendored(&mut project.languages);
        wired(project, test_cache(), Arc::new(FixedClock::at_noon()))
    };
    let args: Vec<OsString> = std::iter::once("spellbook")
        .chain(rest.iter().copied())
        .map(OsString::from)
        .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_with(args, dir, &env, &wire, &mut out, &mut err);
    let said = String::from_utf8(out).unwrap() + &String::from_utf8(err).unwrap();
    (code, said)
}

fn bistro_with_debt() -> tempfile::TempDir {
    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    write_file(dir.path(), CLOSE, SQLITE);
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(status.status.success(), "git {args:?}: {status:?}");
}

fn committed(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "base"]);
}

fn baseline_file(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(".sorcerer/spellbook/baseline.toml")).unwrap()
}

#[test]
fn a_baselined_finding_is_existing_and_does_not_fail() {
    let dir = bistro_with_debt();
    let (code, said) = run(dir.path(), &["check", "--baseline"]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(
        said,
        "baselined 1 finding in .sorcerer/spellbook/baseline.toml\n"
    );
    let text = baseline_file(dir.path());
    assert!(
        text.contains("[[baseline]]") && text.contains(CLOSE),
        "{text}"
    );
    assert!(text.contains("rule = \"commands-use-ports\""), "{text}");

    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 0, "{said}");
    assert!(
        said.starts_with(&format!("· {CLOSE}:2  commands-use-ports — ")),
        "{said}"
    );
    assert!(said.contains("(existing)\n"), "{said}");
    assert!(
        said.ends_with("conventions hold (1 finding, 1 existing, none blocking)\n"),
        "{said}"
    );
    let (_, json) = run(dir.path(), &["check", "--json"]);
    assert!(json.contains("\"existing\": true"), "{json}");
}

#[test]
fn a_new_finding_fails_beside_the_existing_one() {
    let dir = bistro_with_debt();
    run(dir.path(), &["check", "--baseline"]);
    // a second copy in the same file is new: an entry stands for one finding
    write_file(
        dir.path(),
        CLOSE,
        "from dataclasses import dataclass\nimport sqlite3\nimport sqlite3 as db\n",
    );
    write_file(dir.path(), OPEN, SQLITE);
    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains(&format!("✗ {OPEN}:2")), "{said}");
    assert!(said.contains(&format!("✗ {CLOSE}:3")), "{said}");
    assert!(
        said.ends_with("2 blocking findings, 1 existing\n"),
        "{said}"
    );
}

#[test]
fn paid_off_debt_is_said_and_recording_again_drops_it() {
    let dir = bistro_with_debt();
    write_file(dir.path(), OPEN, SQLITE);
    run(dir.path(), &["check", "--baseline"]);
    write_file(dir.path(), OPEN, "from dataclasses import dataclass\n");
    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 0, "{said}");
    assert!(
        said.contains(
            "1 baseline entry is paid off (spellbook check --baseline records less debt)\n"
        ),
        "{said}"
    );
    let (_, said) = run(dir.path(), &["check", "--baseline"]);
    assert_eq!(
        said,
        "baselined 1 finding in .sorcerer/spellbook/baseline.toml (1 paid off, 0 new)\n"
    );
    assert!(!baseline_file(dir.path()).contains(OPEN));
}

#[test]
fn a_broken_baseline_stops_the_check() {
    let dir = bistro_with_debt();
    write_file(
        dir.path(),
        ".sorcerer/spellbook/baseline.toml",
        "[[baseline]]\nline = 3\n",
    );
    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 2, "{said}");
    assert!(
        said.contains(".sorcerer/spellbook/baseline.toml: "),
        "{said}"
    );
}

#[test]
fn baseline_takes_no_files_and_no_since() {
    let dir = bistro_with_debt();
    let (code, said) = run(dir.path(), &["check", "--baseline", CLOSE]);
    assert_eq!(code, 2);
    // the usage line shows the forms apart, not the mix just refused
    assert!(
        said.contains("Usage: spellbook check [--json] [FILES]...\n"),
        "{said}"
    );
    assert!(!said.contains("--baseline [FILES]"), "{said}");
    let (code, _) = run(dir.path(), &["check", "--baseline", "--since", "HEAD"]);
    assert_eq!(code, 2);
    let (code, _) = run(dir.path(), &["check", "--since", "HEAD", CLOSE]);
    assert_eq!(code, 2);
}

#[test]
fn since_keeps_the_findings_on_changed_lines() {
    let dir = bistro_with_debt();
    committed(dir.path());
    // the old finding moves down a line, untouched; a new one is written below it
    write_file(
        dir.path(),
        CLOSE,
        "\"\"\"Close a table.\"\"\"\nfrom dataclasses import dataclass\nimport sqlite3\nfrom sqlite3 import connect\n",
    );
    write_file(dir.path(), OPEN, SQLITE);
    let (code, said) = run(dir.path(), &["check", "--since", "HEAD"]);
    assert_eq!(code, 1, "{said}");
    assert!(!said.contains(&format!("{CLOSE}:3")), "{said}");
    assert!(said.contains(&format!("✗ {CLOSE}:4")), "{said}");
    assert!(said.contains(&format!("✗ {OPEN}:2")), "{said}");
    let (_, json) = run(dir.path(), &["check", "--since", "HEAD", "--json"]);
    assert!(json.contains("\"since\": \"HEAD\""), "{json}");
    let (code, all) = run(dir.path(), &["check"]);
    assert_eq!(code, 1);
    assert!(all.contains(&format!("{CLOSE}:3")), "{all}");
}

#[test]
fn since_with_nothing_changed_holds() {
    let dir = bistro_with_debt();
    committed(dir.path());
    let (code, said) = run(dir.path(), &["check", "--since", "HEAD"]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(said, "conventions hold (no findings)\n");
}

#[test]
fn since_a_commit_git_does_not_know_stops() {
    let dir = bistro_with_debt();
    committed(dir.path());
    let (code, said) = run(dir.path(), &["check", "--since", "no-such-rev"]);
    assert_eq!(code, 2, "{said}");
    assert!(said.contains("no-such-rev"), "{said}");
}

#[test]
fn a_rule_setting_makes_it_a_warning_or_turns_it_off() {
    let dir = bistro_with_debt();
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook.rules]\ncommands-use-ports = \"warn\"\n",
    );
    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 0, "{said}");
    assert!(said.starts_with(&format!("! {CLOSE}:2")), "{said}");

    // off: not run, and an allow naming it is still a known rule
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook.rules]\ncommands-use-ports = \"off\"\n",
    );
    write_file(
        dir.path(),
        OPEN,
        "# spellbook:allow commands-use-ports — migration\nimport sqlite3\n",
    );
    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(said, "conventions hold (no findings)\n");
}

#[test]
fn a_rule_setting_for_no_rule_stops_and_names_it() {
    let dir = bistro_with_debt();
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook.rules]\ncomands-use-ports = \"warn\"\n",
    );
    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 2, "{said}");
    assert!(
        said.contains("[spellbook.rules]: `comands-use-ports` names no rule (the rules are: "),
        "{said}"
    );
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook.rules]\ncommands-use-ports = \"loud\"\n",
    );
    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 2, "{said}");
    assert!(
        said.contains("[spellbook.rules] commands-use-ports = \"loud\" is not a setting"),
        "{said}"
    );
}

#[test]
fn since_calls_a_pasted_copy_of_baselined_code_new() {
    let dir = bistro_with_debt();
    run(dir.path(), &["check", "--baseline"]);
    committed(dir.path());
    // the copy goes above the original, so the original is the line that moved
    write_file(
        dir.path(),
        CLOSE,
        "from dataclasses import dataclass\nimport sqlite3\nimport sqlite3\n",
    );
    let (code, all) = run(dir.path(), &["check"]);
    assert_eq!(code, 1, "{all}");
    let (code, said) = run(dir.path(), &["check", "--since", "HEAD"]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains(&format!("✗ {CLOSE}:")), "{said}");
    assert!(!said.contains("(existing)"), "{said}");
    assert!(said.ends_with("1 blocking finding\n"), "{said}");
}

const IN_HANDLE: &str = "import sqlite3\n\n\ndef handle():\n    return sqlite3.connect(\"a\")\n\n\ndef new():\n    return None\n";

#[test]
fn a_baseline_entry_does_not_excuse_the_same_code_in_another_definition() {
    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    write_file(dir.path(), CLOSE, IN_HANDLE);
    let (_, said) = run(dir.path(), &["check", "--baseline"]);
    assert!(said.starts_with("baselined 2 findings"), "{said}");
    // the baselined call leaves handle, and the same line is written in new
    write_file(
        dir.path(),
        CLOSE,
        "import sqlite3\n\n\ndef handle():\n    return None\n\n\ndef new():\n    return sqlite3.connect(\"a\")\n",
    );
    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains(&format!("✗ {CLOSE}:9")), "{said}");
}

#[test]
fn a_baselined_finding_stays_existing_through_edits_elsewhere() {
    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    write_file(dir.path(), CLOSE, IN_HANDLE);
    run(dir.path(), &["check", "--baseline"]);
    // a docstring above, the definitions swapped, and new's body edited
    write_file(
        dir.path(),
        CLOSE,
        "\"\"\"Close a table.\"\"\"\nimport sqlite3\n\n\ndef new():\n    seats = 4\n    return seats\n\n\ndef handle():\n    return   sqlite3.connect(\"a\")\n",
    );
    let (code, said) = run(dir.path(), &["check"]);
    assert_eq!(code, 0, "{said}");
    assert!(
        said.ends_with("conventions hold (2 findings, 2 existing, none blocking)\n"),
        "{said}"
    );
}
