//! The command line, on a copy of bistro with the vendored grammars and a fixed clock.

mod support;

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::domain::project::Project;
use spellbook::adapters::wiring::{Wired, wired};
use spellbook::cli::main_with;
use support::*;

fn args(rest: &[&str]) -> Vec<OsString> {
    std::iter::once("spellbook")
        .chain(rest.iter().copied())
        .map(OsString::from)
        .collect()
}

/// Runs a command in a directory: the exit code, what it printed and what it said.
fn run(dir: &Path, rest: &[&str]) -> (i32, String, String) {
    let env = MapEnv::default().with_user_dirs(dir);
    run_with_env(dir, &env, rest)
}

fn run_with_env(dir: &Path, env: &MapEnv, rest: &[&str]) -> (i32, String, String) {
    let wire = |mut project: Project| -> sorcerer_core::Result<Wired> {
        point_at_vendored(&mut project.languages);
        wired(project, test_cache(), Arc::new(FixedClock::at_noon()))
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_with(args(rest), dir, env, &wire, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

const MAIN: &str = "src/bistro/booking/commands/cancel_table.py";

#[test]
fn version_prints_the_name() {
    let dir = tempfile::tempdir().unwrap();
    let (code, out, _) = run(dir.path(), &["--version"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("spellbook "));
}

#[test]
fn list_shows_the_command_generator() {
    let dir = bistro_project();
    let (code, out, err) = run(dir.path(), &["list"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "command  A command of a service, with its test  inputs: app, service, command\n"
    );
}

#[test]
fn list_shows_a_broken_generator_as_a_row() {
    let dir = bistro_project();
    write_generator(dir.path(), "broken", "nme = \"x\"\n", &[]);
    let (_, out, _) = run(dir.path(), &["list"]);
    assert!(out.starts_with("! broken"), "{out}");
}

#[test]
fn new_with_plan_writes_nothing_and_shows_three_holes() {
    let dir = bistro_project();
    let (code, out, err) = run(
        dir.path(),
        &["new", "command", "--set", "command=cancel_table", "--plan"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("command (2 files, 3 holes)\n"), "{out}");
    assert!(
        out.contains(&format!("  field   {MAIN}:10  ident  its first input\n")),
        "{out}"
    );
    assert!(out.contains("handle"), "{out}");
    assert!(
        out.contains("--- src/bistro/booking/commands/cancel_table__test.py"),
        "{out}"
    );
    assert!(out.contains("(a plan: nothing was written)"), "{out}");
    assert!(!dir.path().join(MAIN).exists());
}

#[test]
fn new_writes_two_files_and_says_the_skipped_one() {
    let dir = bistro_project();
    let (code, out, err) = run(
        dir.path(),
        &["new", "command", "--set", "command=cancel_table"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(&format!("  + {MAIN}")), "{out}");
    assert!(
        out.contains("  = src/bistro/booking/commands/__init__.py"),
        "{out}"
    );
    assert!(out.contains("(exists, skipped)"), "{out}");
    assert!(
        out.contains("(filled from the inputs: purpose \"Cancel table.\""),
        "{out}"
    );
    assert!(dir.path().join(MAIN).exists());
    assert!(
        dir.path()
            .join(".sorcerer/state/spellbook/generations.jsonl")
            .exists()
    );
}

#[test]
fn new_json_parses_and_names_the_holes() {
    let dir = bistro_project();
    let (code, out, err) = run(
        dir.path(),
        &["new", "command", "--set", "command=cancel_table", "--json"],
    );
    assert_eq!(code, 0, "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["applied"], true);
    assert_eq!(report["written"][0], MAIN);
    let names: Vec<&str> = report["holes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hole| hole["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["field", "type", "handle"]);
    assert_eq!(report["plan"]["files"][1]["status"], "new");
}

#[test]
fn a_missing_input_exits_2_with_prompt_and_choices() {
    let dir = bistro_project();
    write_file(dir.path(), "src/bistro/kitchen/models/__init__.py", "");
    let (code, out, err) = run(
        dir.path(),
        &["new", "command", "--set", "command=cancel_table"],
    );
    assert_eq!(code, 2);
    assert_eq!(
        err,
        "spellbook: service: the service it belongs to (booking, kitchen)\n  give it with --set service=VALUE\n"
    );
    assert!(out.is_empty());
    let (code, out, _) = run(
        dir.path(),
        &["new", "command", "--set", "command=x", "--json"],
    );
    assert_eq!(code, 2);
    let stop: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(stop["input"], "service");
    assert_eq!(stop["choices"][1], "kitchen");
}

#[test]
fn holes_lists_four_places_then_three() {
    let dir = bistro_project();
    run(
        dir.path(),
        &["new", "command", "--set", "command=cancel_table"],
    );
    let (code, out, _) = run(dir.path(), &["holes"]);
    assert_eq!(code, 0);
    assert!(out.ends_with("4 open holes\n"), "{out}");
    let text = std::fs::read_to_string(dir.path().join(MAIN)).unwrap();
    let filled = text.replace(
        "raise NotImplementedError(\"hole handle: its first case, from the test file beside it\")",
        "return Ok(None)",
    );
    std::fs::write(dir.path().join(MAIN), filled).unwrap();
    let (_, out, _) = run(dir.path(), &["holes"]);
    assert!(out.ends_with("3 open holes\n"), "{out}");
    assert!(!out.contains("handle"), "{out}");
    let (_, out, _) = run(dir.path(), &["holes", MAIN, "--json"]);
    let rows: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
}

#[test]
fn doctor_says_holes_are_filled_by_hand_without_conjure() {
    let dir = bistro_project();
    let (code, out, err) = run(dir.path(), &["doctor"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("generators   ok    command\n"), "{out}");
    assert!(
        out.contains("holes are filled by hand; install conjure"),
        "{out}"
    );
    assert!(
        out.contains("python (holes: block, expr, ident, type)"),
        "{out}"
    );
    let env = MapEnv::default()
        .with_user_dirs(dir.path())
        .on_path("conjure");
    let (_, out, _) = run_with_env(dir.path(), &env, &["doctor"]);
    assert!(out.contains("holes can be conjured in the editor"), "{out}");
}

#[test]
fn trust_and_revoke_round_trip() {
    let dir = bistro_project();
    let env = MapEnv::default().with_user_dirs(dir.path());
    let (code, out, _) = run_with_env(dir.path(), &env, &["trust"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("trusted: "), "{out}");
    let list = sorcerer_core::adapters::trust_list::TrustList::user(&env).unwrap();
    let root = Project::open(dir.path()).unwrap().root;
    assert!(list.is_trusted(&root));
    let (_, out, _) = run_with_env(dir.path(), &env, &["trust", "--revoke"]);
    assert!(out.starts_with("no longer trusted: "), "{out}");
    assert!(!list.is_trusted(&root));
}

#[test]
fn show_lists_inputs_and_choices() {
    let dir = bistro_project();
    let (code, out, err) = run(dir.path(), &["show", "command"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("command (python): Command\n"), "{out}");
    assert!(out.contains("choices now: booking"), "{out}");
    assert!(
        out.contains("handle   block  its first case, from the test file beside it"),
        "{out}"
    );
    assert!(out.contains("examples:\n  spellbook new command"), "{out}");
}

fn bistro_with_a_sqlite_command() -> tempfile::TempDir {
    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    write_file(
        dir.path(),
        "src/bistro/booking/commands/close_table.py",
        "from dataclasses import dataclass\nimport sqlite3\n",
    );
    dir
}

#[test]
fn spellbook_off_in_the_config_stops_every_command_but_doctor() {
    let dir = bistro_with_a_sqlite_command();
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook]\nenabled = false\n",
    );
    for command in [
        &["check"][..],
        &["list"],
        &["holes"],
        &["new", "command", "--plan"],
    ] {
        let (code, out, err) = run(dir.path(), command);
        assert_eq!(code, 2, "{command:?} {out}{err}");
        assert!(
            err.contains("spellbook is off in this project ([spellbook] enabled = false)"),
            "{err}"
        );
    }
    let (code, _, _) = run(dir.path(), &["doctor"]);
    assert_eq!(code, 0);
}

#[test]
fn check_exits_0_with_a_warning_and_1_with_an_error() {
    let dir = bistro_with_a_sqlite_command();
    let (code, out, err) = run(dir.path(), &["check"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.starts_with("! src/bistro/booking/commands/close_table.py:2  commands-use-ports — Commands and queries talk to ports"),
        "{out}"
    );
    assert!(
        out.ends_with("conventions hold (1 finding, none blocking)\n"),
        "{out}"
    );
    let rules = std::fs::read_to_string(dir.path().join(".sorcerer/spellbook/rules.toml")).unwrap();
    let strict = rules.replacen("severity = \"warn\"", "severity = \"error\"", 1);
    write_file(dir.path(), ".sorcerer/spellbook/rules.toml", &strict);
    let (code, out, _) = run(dir.path(), &["check"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("✗ "), "{out}");
    assert!(out.ends_with("1 blocking finding\n"), "{out}");
}

#[test]
fn check_takes_files_as_dot_slash_or_absolute_paths() {
    let dir = bistro_with_a_sqlite_command();
    let close = "src/bistro/booking/commands/close_table.py";
    let absolute = dir.path().join(close).display().to_string();
    for given in [format!("./{close}"), absolute] {
        let (code, out, err) = run(dir.path(), &["check", &given]);
        assert_eq!(code, 0, "{out}{err}");
        assert!(
            out.contains(&format!("{close}:2  commands-use-ports")),
            "{given}: {out}"
        );
    }
    let report =
        std::fs::read_to_string(dir.path().join(".sorcerer/state/spellbook/report.json")).unwrap();
    assert!(!report.contains("\"./"), "{report}");
    let (code, _, err) = run(dir.path(), &["check", "/somewhere/else.py"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("outside the project"), "{err}");
}

#[test]
fn check_with_no_findings_holds_and_with_no_rules_says_so() {
    let dir = bistro_project();
    let (code, out, _) = run(dir.path(), &["check"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("conventions hold (no findings)\n"), "{out}");
    assert!(out.contains("no rules yet"), "{out}");
}

#[test]
fn check_json_equals_the_report() {
    let dir = bistro_with_a_sqlite_command();
    let (code, out, _) = run(dir.path(), &["check", "--json"]);
    assert_eq!(code, 0);
    let printed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let saved =
        std::fs::read_to_string(dir.path().join(".sorcerer/state/spellbook/report.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(printed, saved["data"]);
    assert_eq!(printed["findings"][0]["line"], 2);
}

#[test]
fn check_on_named_files_checks_only_those() {
    let dir = bistro_with_a_sqlite_command();
    write_file(
        dir.path(),
        "src/bistro/booking/commands/open_table.py",
        "import sqlite3\n",
    );
    let (_, out, _) = run(
        dir.path(),
        &["check", "src/bistro/booking/commands/open_table.py"],
    );
    assert!(out.contains("open_table.py:1"), "{out}");
    assert!(!out.contains("close_table"), "{out}");
}

#[test]
fn a_broken_rules_file_stops_check_with_exit_2() {
    let dir = bistro_project();
    write_file(
        dir.path(),
        ".sorcerer/spellbook/rules.toml",
        "[rules.a]\nfiles = \"x\"\n",
    );
    let (code, _, err) = run(dir.path(), &["check"]);
    assert_eq!(code, 2);
    assert!(
        err.contains("rules.toml: rule a: `message` is missing"),
        "{err}"
    );
}

#[test]
fn doctor_names_the_rules_file_and_the_report() {
    let dir = bistro_with_a_sqlite_command();
    let (code, out, err) = run(dir.path(), &["doctor"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("rules        ok    4 rules: adapters-in-main, commands-use-ports, failures-returned, web-uses-commands\n"),
        "{out}"
    );
    assert!(
        out.contains("report       ok    no report.json yet"),
        "{out}"
    );
    run(dir.path(), &["check"]);
    let (_, out, _) = run(dir.path(), &["doctor"]);
    assert!(
        out.contains("report.json version 1, 1 finding at the last check, 2026-11-22 12:00 UTC"),
        "{out}"
    );
    write_file(
        dir.path(),
        ".sorcerer/spellbook/rules.toml",
        "[rules.a]\nfiles = \"x\"\n",
    );
    let (code, out, _) = run(dir.path(), &["doctor"]);
    assert_eq!(code, 2);
    assert!(
        out.contains("rules        FAIL  .sorcerer/spellbook/rules.toml: rule a:"),
        "{out}"
    );
}
