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
fn a_hole_whose_file_is_briefly_gone_comes_back_with_it() {
    let dir = bistro_project();
    run(
        dir.path(),
        &["new", "command", "--set", "command=cancel_table"],
    );
    let kept = std::fs::read_to_string(dir.path().join(MAIN)).unwrap();
    std::fs::remove_file(dir.path().join(MAIN)).unwrap();
    let (_, out, _) = run(dir.path(), &["holes"]);
    assert!(out.ends_with("1 open hole\n"), "{out}");
    std::fs::write(dir.path().join(MAIN), kept).unwrap();
    let (_, out, _) = run(dir.path(), &["holes"]);
    assert!(out.ends_with("4 open holes\n"), "{out}");
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
fn check_exits_1_with_an_error_and_0_with_only_warnings() {
    let dir = bistro_with_a_sqlite_command();
    let (code, out, _) = run(dir.path(), &["check"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("✗ "), "{out}");
    assert!(out.ends_with("1 blocking finding\n"), "{out}");
    let rules = std::fs::read_to_string(dir.path().join(".sorcerer/spellbook/rules.toml")).unwrap();
    let lenient = rules.replacen(
        "[rules.commands-use-ports]\n",
        "[rules.commands-use-ports]\nseverity = \"warn\"\n",
        1,
    );
    write_file(dir.path(), ".sorcerer/spellbook/rules.toml", &lenient);
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
}

#[test]
fn check_takes_files_as_dot_slash_or_absolute_paths() {
    let dir = bistro_with_a_sqlite_command();
    let close = "src/bistro/booking/commands/close_table.py";
    let absolute = dir.path().join(close).display().to_string();
    for given in [format!("./{close}"), absolute] {
        let (code, out, err) = run(dir.path(), &["check", &given]);
        assert_eq!(code, 1, "{out}{err}");
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
fn check_json_is_the_report_with_each_findings_fixes() {
    let dir = bistro_with_a_sqlite_command();
    let (code, out, _) = run(dir.path(), &["check", "--json"]);
    assert_eq!(code, 1);
    let mut printed: serde_json::Value = serde_json::from_str(&out).unwrap();
    // The fixes are offered as the rules are now, so report.json does not keep them.
    for finding in printed["findings"].as_array_mut().unwrap() {
        finding.as_object_mut().unwrap().remove("fixes");
    }
    let saved =
        std::fs::read_to_string(dir.path().join(".sorcerer/state/spellbook/report.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(printed, saved["data"]);
    assert_eq!(printed["findings"][0]["line"], 2);
}

#[test]
fn check_texts_checks_what_is_not_on_disk_and_saves_nothing() {
    let dir = bistro_with_a_sqlite_command();
    let close = "src/bistro/booking/commands/close_table.py";
    let open = "src/bistro/booking/commands/open_table.py";
    let asked = serde_json::json!({ "files": [
        { "path": close, "text": "from dataclasses import dataclass\n" },
        { "path": open, "text": "import os\nimport sqlite3\n" },
    ] });
    write_file(dir.path(), "texts.json", &asked.to_string());
    let (code, out, err) = run(dir.path(), &["check", "--texts", "texts.json", "--json"]);
    assert_eq!(code, 1, "{out}{err}");
    let printed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let findings = printed["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "{out}");
    assert_eq!(findings[0]["file"], open);
    assert_eq!(findings[0]["line"], 2);
    assert_eq!(findings[0]["severity"], "error");
    assert!(!dir.path().join(open).exists(), "nothing is written");
    assert!(
        !dir.path()
            .join(".sorcerer/state/spellbook/report.json")
            .exists(),
        "no report is saved"
    );
    let outside = serde_json::json!({ "files": [{ "path": "../x.py", "text": "" }] });
    write_file(dir.path(), "texts.json", &outside.to_string());
    let (code, _, err) = run(dir.path(), &["check", "--texts", "texts.json"]);
    assert_eq!(code, 2);
    assert!(err.contains("../x.py is outside the project"), "{err}");
}

#[test]
fn rules_lists_the_rules_that_look_at_a_file() {
    let dir = bistro_with_a_sqlite_command();
    let (code, out, err) = run(
        dir.path(),
        &[
            "rules",
            "--json",
            "src/bistro/booking/commands/close_table.py",
        ],
    );
    assert_eq!(code, 0, "{out}{err}");
    let printed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let rules = printed["rules"].as_array().unwrap();
    let ports = rules
        .iter()
        .find(|rule| rule["id"] == "commands-use-ports")
        .expect(&out);
    assert_eq!(ports["severity"], "error");
    write_file(dir.path(), "notes/readme.txt", "hello\n");
    let (code, out, err) = run(dir.path(), &["rules", "notes/readme.txt"]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(out, "no rules look at notes/readme.txt\n");
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

const CLOSE_TABLE: &str = "src/bistro/booking/commands/close_table.py";

/// Bistro with its rules and generators, a `close_table` command made by `new`, and
/// `import sqlite3` added by hand on line 4.
fn bistro_with_sqlite_in_close_table() -> tempfile::TempDir {
    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    let (code, _, err) = run(
        dir.path(),
        &["new", "command", "--set", "command=close_table"],
    );
    assert_eq!(code, 0, "{err}");
    let path = dir.path().join(CLOSE_TABLE);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    lines.insert(3, "import sqlite3");
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
    dir
}

#[test]
fn the_fix_session_of_the_design_runs_end_to_end() {
    let dir = bistro_with_sqlite_in_close_table();
    let at = format!("{CLOSE_TABLE}:4");

    let (code, out, _) = run(dir.path(), &["check"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains(&format!(
            "✗ {CLOSE_TABLE}:4  commands-use-ports — Commands and queries talk to ports, not the database (python-htmx: ports & adapters)  (fixes: Use a port the service has · Add a port and use it)\n"
        )),
        "{out}"
    );
    let (code, json, _) = run(dir.path(), &["check", "--json"]);
    assert_eq!(code, 1);
    assert!(
        json.contains("\"missing\": [\n            \"port\"\n          ]"),
        "{json}"
    );

    let (code, _, err) = run(dir.path(), &["fix", &at]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("table_repository"), "{err}");
    assert!(err.contains("give it with --set port=VALUE"), "{err}");

    let (code, out, err) = run(
        dir.path(),
        &["fix", &at, "--set", "port=table_repository", "--plan"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("Use a port the service has: use-port (2 files changed, 0 new, 0 holes)\n"),
        "{out}"
    );
    assert!(out.contains(&format!("  ~ {CLOSE_TABLE}  ")), "{out}");
    assert!(out.contains(&format!("--- {CLOSE_TABLE}\n@@ ")), "{out}");
    assert!(out.contains("-import sqlite3\n"), "{out}");
    assert!(
        out.contains("+    return CloseTable(table_repository=InMemoryTableRepository())\n"),
        "{out}"
    );
    assert!(out.ends_with("nothing written (--plan)\n"), "{out}");
    assert!(
        std::fs::read_to_string(dir.path().join(CLOSE_TABLE))
            .unwrap()
            .contains("import sqlite3")
    );

    let (code, out, err) = run(dir.path(), &["fix", &at, "--set", "port=table_repository"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.ends_with("applied: 2 files changed, 0 written\n"),
        "{out}"
    );
    let (_, out, _) = run(dir.path(), &["check", CLOSE_TABLE]);
    assert!(!out.contains("commands-use-ports"), "{out}");

    let (code, out, err) = run(dir.path(), &["fix", &at, "--set", "port=table_repository"]);
    assert_eq!(code, 3, "{out}{err}");
    assert_eq!(out, "nothing to do: the fix is already applied\n");
}

/// The starter corpus's project: empty but for `.sorcerer/spellbook/` (generators `app`,
/// `migration`, `page`).
fn starter_project() -> tempfile::TempDir {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/spellbook/starter");
    let corpus: CorpusFile = read_toml(&dir.join("corpus.toml"));
    project_of(&dir, &corpus, Base::Overlaid)
}

const FONT: &str = "static/fonts/mark.woff2";

#[test]
fn new_in_an_empty_project_copies_the_font_and_prints_the_chains_notes_once() {
    let dir = starter_project();
    let (code, out, err) = run(dir.path(), &["new", "app", "--set", "app=tracker"]);
    assert_eq!(code, 0, "{err}");
    let font = std::fs::read(dir.path().join(FONT)).unwrap();
    let source = std::fs::read(
        dir.path()
            .join(".sorcerer/spellbook/generators/app/files/fonts/mark.woff2"),
    )
    .unwrap();
    assert_eq!(font, source);
    let font_row = out.lines().find(|line| line.contains(FONT)).unwrap();
    assert!(
        font_row.ends_with(&format!("(binary, {} bytes)", font.len())),
        "{out}"
    );
    // the plan's clock names the migration the app's [[uses]] plans
    assert!(
        dir.path()
            .join("migrations/20261122120000_init.sql")
            .is_file()
    );
    // app's notes, then migration's, each line once
    assert!(
        out.ends_with("cd tracker && cp .env.example .env\ncargo run\n"),
        "{out}"
    );
}

#[test]
fn a_timestamp_given_names_the_whole_chains_files_and_json_carries_the_notes() {
    let dir = starter_project();
    let (code, out, err) = run(
        dir.path(),
        &[
            "new",
            "app",
            "--set",
            "app=tracker",
            "--set",
            "timestamp=20260101093000",
            "--plan",
            "--json",
        ],
    );
    assert_eq!(code, 0, "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    let paths: Vec<&str> = report["plan"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect();
    assert!(
        paths.contains(&"migrations/20260101093000_init.sql"),
        "{paths:?}"
    );
    let font = &report["plan"]["files"][3];
    assert_eq!(font["path"], FONT);
    assert!(font["binary"].as_u64().unwrap() > 0, "{font}");
    assert_eq!(
        report["plan"]["notes"],
        serde_json::json!(["cd tracker && cp .env.example .env", "cargo run"])
    );
}

#[test]
fn a_plan_shows_a_binary_file_by_its_size() {
    let dir = starter_project();
    let (code, out, err) = run(dir.path(), &["new", "app", "--set", "app=x", "--plan"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(&format!("--- {FONT}\n(binary, ")), "{out}");
    assert!(!dir.path().join(FONT).exists());
}

/// Runs git in a folder, with no signing and a fixed author; what it printed.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.email=pack@example.com",
            "-c",
            "user.name=pack",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// A local pack repository: `pack.toml` (when named), a config, a rules file and a state folder,
/// tagged `v1`, then a second commit on top. Returns the folder and v1's commit.
fn pack_repository(name: Option<&str>) -> (tempfile::TempDir, String) {
    let pack = tempfile::tempdir().unwrap();
    let root = pack.path();
    git(root, &["init", "-q"]);
    if let Some(name) = name {
        write_file(root, "pack.toml", &format!("name = \"{name}\"\n"));
    }
    write_file(
        root,
        ".sorcerer/config.toml",
        "[spellbook]\nenabled = true\n",
    );
    write_file(root, ".sorcerer/spellbook/rules.toml", "# v1\n");
    write_file(root, ".sorcerer/state/remembered.json", "{}\n");
    git(root, &["add", "-A", "-f"]);
    git(root, &["commit", "-q", "-m", "v1"]);
    git(root, &["tag", "v1"]);
    let v1 = git(root, &["rev-parse", "HEAD"]);
    write_file(root, ".sorcerer/spellbook/rules.toml", "# v2\n");
    git(root, &["commit", "-q", "-a", "-m", "v2"]);
    (pack, v1)
}

#[test]
fn init_pack_copies_the_packs_sorcerer_keeps_the_projects_own_and_records_the_commit() {
    let (pack, v1) = pack_repository(Some("rust-web"));
    let project = tempfile::tempdir().unwrap();
    write_file(project.path(), ".sorcerer/config.toml", "# mine\n");
    let address = format!("{}@v1", pack.path().display());
    let (code, out, err) = run(project.path(), &["init", "--pack", &address]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with(&format!("pack rust-web at {}: 1 file copied", &v1[..12])),
        "{out}"
    );
    assert!(
        out.contains("= .sorcerer/config.toml  (the project's own, kept)"),
        "{out}"
    );
    let sorcerer = project.path().join(".sorcerer");
    assert_eq!(
        std::fs::read_to_string(sorcerer.join("config.toml")).unwrap(),
        "# mine\n"
    );
    assert_eq!(
        std::fs::read_to_string(sorcerer.join("spellbook/rules.toml")).unwrap(),
        "# v1\n"
    );
    assert!(!sorcerer.join("state/remembered.json").exists());
    let record: toml::Table = std::fs::read_to_string(sorcerer.join("pack.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(record["name"].as_str(), Some("rust-web"));
    assert_eq!(record["repo"].as_str(), Some(pack.path().to_str().unwrap()));
    assert_eq!(record["rev"].as_str(), Some(v1.as_str()));
}

#[test]
fn init_pack_without_a_revision_takes_the_default_branch_and_names_the_pack_by_its_folder() {
    let (pack, v1) = pack_repository(None);
    let project = tempfile::tempdir().unwrap();
    let address = pack.path().display().to_string();
    let (code, _, err) = run(project.path(), &["init", "--pack", &address]);
    assert_eq!(code, 0, "{err}");
    let sorcerer = project.path().join(".sorcerer");
    assert_eq!(
        std::fs::read_to_string(sorcerer.join("spellbook/rules.toml")).unwrap(),
        "# v2\n"
    );
    let record: toml::Table = std::fs::read_to_string(sorcerer.join("pack.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let folder = pack.path().file_name().unwrap().to_str().unwrap();
    assert_eq!(record["name"].as_str(), Some(folder));
    assert_ne!(record["rev"].as_str(), Some(v1.as_str()));
}

#[test]
fn init_pack_refuses_a_project_with_a_spellbook_and_a_revision_git_lacks() {
    let (pack, _) = pack_repository(Some("rust-web"));
    let project = tempfile::tempdir().unwrap();
    write_file(project.path(), ".sorcerer/spellbook/rules.toml", "# mine\n");
    let address = pack.path().display().to_string();
    let (code, _, err) = run(project.path(), &["init", "--pack", &address]);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        "spellbook: the project has .sorcerer/spellbook/ already; a pack is copied only into a project without one (nothing was written)\n"
    );
    assert!(!project.path().join(".sorcerer/pack.toml").exists());

    let empty = tempfile::tempdir().unwrap();
    let address = format!("{}@no-such-tag", pack.path().display());
    let (code, _, err) = run(empty.path(), &["init", "--pack", &address]);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("spellbook: the pack could not be fetched: git checkout: "),
        "{err}"
    );
    assert!(!empty.path().join(".sorcerer").exists());
}
