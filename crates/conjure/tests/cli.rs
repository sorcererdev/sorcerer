//! The command line, over a stub writer and the vendored grammars.

mod support;

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use conjure::adapters::project_context::FilesContext;
use conjure::adapters::wiring::Wired;
use conjure::cli::main_with;
use conjure::ports::writer::{Writer, Written};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::GrammarSource;
use sorcerer_core::ports::ledger::Ledger;
use support::*;

fn args(rest: &[&str]) -> Vec<OsString> {
    std::iter::once("conjure")
        .chain(rest.iter().copied())
        .map(OsString::from)
        .collect()
}

/// A writer that gives a fixed reply and records its errand in the project's ledger, as the
/// live writers do.
struct LedgerWriter {
    reply: String,
    ledger: Arc<JsonlLedger>,
}

impl Writer for LedgerWriter {
    fn write(&self, _: &str, _: &Cancel) -> sorcerer_models::Result<Written> {
        let errand = errand("conjure", "haiku", 0.001);
        self.ledger.append(&errand).unwrap();
        Ok(Written {
            text: self.reply.clone(),
            errand,
        })
    }

    fn model(&self) -> &str {
        "haiku"
    }
}

/// Runs the command in DIR with the stub writer; the exit code, what it printed and what it said.
fn run(dir: &Path, rest: &[&str], reply: &str) -> (i32, String, String) {
    let env = MapEnv::default().with_user_dirs(dir);
    run_with_env(dir, &env, rest, reply)
}

/// [`run`] with an environment given.
fn run_with_env(dir: &Path, env: &MapEnv, rest: &[&str], reply: &str) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let reply = reply.to_string();
    let wire = move |mut project: sorcerer_core::domain::project::Project,
                     _: &conjure::domain::config::ConjureConfig,
                     _: conjure::domain::record::Attempt| {
        for name in ["python", "rust"] {
            project
                .languages
                .override_grammar(name, GrammarSource::Local(vendored(name)));
        }
        let ledger = Arc::new(JsonlLedger::for_project(&project)?);
        let project = Arc::new(project);
        Ok(Wired {
            view: Arc::new(FilesView::new(project.clone(), test_cache())),
            context: Box::new(FilesContext {
                project: project.clone(),
                view: Arc::new(FilesView::new(project.clone(), test_cache())),
                cache: test_cache(),
                fences: Fences::for_project(&project).unwrap(),
            }),
            writer: Box::new(LedgerWriter {
                reply: reply.clone(),
                ledger: ledger.clone(),
            }),
            checker: Box::new(NotChecking),
            ledger,
            name_case: conjure::domain::edits::NameCase::Matters,
            clock: Arc::new(FixedClock::at("2026-11-22T12:00:00Z".parse().unwrap())),
            project: (*project).clone(),
        })
    };
    let code = main_with(args(rest), dir, env, &wire, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn conjure_at_a_line_prints_the_goal_and_the_diff_and_writes_nothing() {
    let d = cart_project();
    let (code, out, err) = run(d.path(), &["src/shop/cart.py:3"], CART_REPLY);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("goal: fill total"), "{out}");
    assert!(out.contains("--- src/shop/cart.py"), "{out}");
    assert!(out.contains("-        pass\n+        return 0"), "{out}");
    assert!(
        out.contains("--- src/shop/cart__test.py (new file)"),
        "{out}"
    );
    assert!(out.contains("parses"), "{out}");
    assert!(out.contains("haiku · $0.001"), "{out}");
    assert!(
        std::fs::read_to_string(d.path().join("src/shop/cart.py"))
            .unwrap()
            .contains("pass")
    );
    assert!(!d.path().join("src/shop/cart__test.py").exists());
    let ledger =
        JsonlLedger::for_project(&sorcerer_core::domain::project::Project::open(d.path()).unwrap())
            .unwrap();
    assert_eq!(ledger.read().unwrap().errands.len(), 1);
    assert!(
        !d.path()
            .join(".sorcerer/state/conjure/changes.jsonl")
            .exists()
    );
}

#[test]
fn a_range_and_an_absolute_path_are_read() {
    let d = cart_project();
    let absolute = std::fs::canonicalize(d.path().join("src/shop/cart.py")).unwrap();
    let target = format!("{}:5-6", absolute.display());
    let (code, out, err) = run(d.path(), &[&target], "<goal>nothing</goal>");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("no change · nothing"), "{out}");
}

#[test]
fn a_cursor_outside_a_unit_exits_2_with_the_sentence() {
    let d = cart_project();
    write(d.path(), "src/shop/notes.py", "# comment\n");
    let (code, out, err) = run(d.path(), &["src/shop/notes.py:1"], CART_REPLY);
    assert_eq!(code, 2);
    assert!(out.is_empty(), "{out}");
    assert!(
        err.contains("put the cursor in a function to conjure"),
        "{err}"
    );
}

#[test]
fn a_target_without_a_line_or_with_a_missing_file_exits_2() {
    let d = cart_project();
    let (code, _, err) = run(d.path(), &["src/shop/cart.py"], CART_REPLY);
    assert_eq!(code, 2);
    assert!(err.contains("FILE:LINE"), "{err}");
    let (code, _, err) = run(d.path(), &["src/shop/missing.py:1"], CART_REPLY);
    assert_eq!(code, 2);
    assert!(err.contains("missing.py"), "{err}");
}

#[test]
fn dropped_paths_and_a_file_that_does_not_parse_are_said() {
    let d = cart_project();
    let reply = "<goal>g</goal>\n<file path=\"../x.py\">\nx\n</file>\n<file path=\"src/shop/cart.py\">\nclass Cart:\n    def total(self:\n        return 0\n</file>\n";
    let (code, out, _) = run(d.path(), &["src/shop/cart.py:3"], reply);
    assert_eq!(code, 0);
    assert!(out.contains("does not parse (line "), "{out}");
    assert!(out.contains("not written: ../x.py"), "{out}");
}

#[test]
fn version_and_help_exit_0() {
    let d = cart_project();
    let (code, out, _) = run(d.path(), &["--version"], "");
    assert_eq!(code, 0);
    assert!(out.starts_with("conjure "), "{out}");
}

// ---- doctor, cost, log, trust ----

use conjure::cli::doctor_rows;
use conjure::domain::record::{
    Attempt, ChangeRecord, Choice, RecordedFacts, RecordedFile, Trigger,
};
use conjure::ports::change_store::ChangeStore;
use sorcerer_core::adapters::env::MapEnv as Env;
use sorcerer_core::domain::project::Project;

fn good_env(home: &Path) -> Env {
    Env::default()
        .on_path("git")
        .on_path("claude")
        .with_user_dirs(home)
}

fn rows(dir: &Path, env: &Env) -> Vec<(String, Result<String, String>)> {
    let project = Project::open(dir).unwrap();
    doctor_rows(&project, env, &test_cache_for(&project))
        .into_iter()
        .map(|r| (r.name.to_string(), r.result))
        .collect()
}

/// The vendored grammars, so `doctor` loads no network grammar.
fn test_cache_for(_: &Project) -> sorcerer_core::adapters::grammar::GrammarCache {
    test_cache()
}

fn row(rows: &[(String, Result<String, String>)], name: &str) -> Result<String, String> {
    rows.iter().find(|(n, _)| n == name).unwrap().1.clone()
}

#[test]
fn doctor_lists_each_row_in_order() {
    let d = git_project();
    let home = tempfile::tempdir().unwrap();
    let got = rows(d.path(), &good_env(home.path()));
    let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "git",
            "provider",
            "config",
            "languages",
            "scry findings",
            "spellbook rules",
            "ledger",
            "records",
            "trust"
        ]
    );
    assert!(
        row(&got, "provider")
            .unwrap()
            .starts_with("claude (claude is on PATH); haiku → "),
        "{:?}",
        row(&got, "provider")
    );
    assert!(row(&got, "provider").unwrap().contains(", sonnet → "));
    assert_eq!(
        row(&got, "config").unwrap(),
        "writer haiku, try harder sonnet, $3.00 a day"
    );
    assert_eq!(
        row(&got, "languages").unwrap(),
        "no file here is in a language sorcerer knows"
    );
    assert_eq!(row(&got, "scry findings").unwrap(), "no file");
    assert_eq!(
        row(&got, "spellbook rules").unwrap(),
        "not checked: this project has no .sorcerer/spellbook/rules.toml"
    );
    assert_eq!(row(&got, "ledger").unwrap(), "0 rows, 0 of them conjure's");
    assert_eq!(row(&got, "records").unwrap(), "0 changes, 0 lines skipped");
    assert_eq!(row(&got, "trust").unwrap(), "not needed");
    let none = rows(d.path(), &Env::default());
    assert!(row(&none, "git").unwrap_err().contains("git-scm.com"));
    assert!(row(&none, "provider").is_err());
}

#[test]
fn doctor_says_whether_changes_are_checked_against_spellbooks_rules() {
    let d = git_project();
    write(d.path(), ".sorcerer/spellbook/rules.toml", "");
    let home = tempfile::tempdir().unwrap();
    let got = rows(d.path(), &good_env(home.path()));
    assert_eq!(
        row(&got, "spellbook rules").unwrap(),
        "not checked: spellbook is not installed"
    );
    let got = rows(d.path(), &good_env(home.path()).on_path("spellbook"));
    let found = row(&got, "spellbook rules").unwrap();
    assert!(
        found.ends_with("spellbook: changes are checked against .sorcerer/spellbook/rules.toml"),
        "{found}"
    );
}

#[test]
fn doctor_reads_the_languages_tables_and_the_imports_query() {
    let d = cart_project();
    let home = tempfile::tempdir().unwrap();
    let project_dir = d.path();
    // the vendored grammars stand in for the downloaded ones
    let got = rows_on_vendored(project_dir, &good_env(home.path()));
    assert_eq!(row(&got, "languages").unwrap(), "python (tests, imports)");
    write(
        project_dir,
        ".sorcerer/languages/python/language.toml",
        "[test]\ntests_fro = \"x\"\n",
    );
    let got = rows_on_vendored(project_dir, &good_env(home.path()));
    assert!(row(&got, "languages").unwrap_err().contains("tests_fro"));
}

fn rows_on_vendored(dir: &Path, env: &Env) -> Vec<(String, Result<String, String>)> {
    let mut project = Project::open(dir).unwrap();
    for name in ["python", "rust"] {
        project
            .languages
            .override_grammar(name, GrammarSource::Local(vendored(name)));
    }
    doctor_rows(&project, env, &test_cache())
        .into_iter()
        .map(|r| (r.name.to_string(), r.result))
        .collect()
}

#[test]
fn doctor_says_when_scry_findings_are_another_version() {
    let d = git_project();
    let home = tempfile::tempdir().unwrap();
    write(
        d.path(),
        ".sorcerer/state/scry/findings.json",
        r#"{"version": 1, "data": {}}"#,
    );
    let got = rows(d.path(), &good_env(home.path()));
    assert_eq!(
        row(&got, "scry findings").unwrap(),
        "version 1, and conjure reads version 2"
    );
    write(
        d.path(),
        ".sorcerer/state/scry/findings.json",
        r#"{"version": 2, "data": {"findings": [
        {"id": "a", "issue_type": "bug", "statement": "s", "place": {"file": "a.py", "lines": {"start": 1, "end": 1}}, "status": "open"},
        {"id": "b", "issue_type": "bug", "statement": "s", "place": {"file": "a.py", "lines": {"start": 1, "end": 1}}, "status": "withheld"}]}}"#,
    );
    let got = rows(d.path(), &good_env(home.path()));
    assert_eq!(row(&got, "scry findings").unwrap(), "version 2, 1 open");
}

#[test]
fn doctor_says_try_harder_off() {
    let d = git_project();
    let home = tempfile::tempdir().unwrap();
    write(
        d.path(),
        ".sorcerer/config.toml",
        "[conjure]\ntry_harder = \"\"\n",
    );
    let got = rows(d.path(), &good_env(home.path()));
    assert!(
        row(&got, "provider").unwrap().ends_with(", try harder off"),
        "{:?}",
        row(&got, "provider")
    );
    assert_eq!(
        row(&got, "config").unwrap(),
        "writer haiku, try harder off, $3.00 a day"
    );
}

#[test]
fn doctor_exits_2_when_a_row_fails_and_prints_rows() {
    let d = git_project();
    let (code, out, _) = run_with_env(d.path(), &Env::default(), &["doctor"], "");
    assert_eq!(code, 2);
    assert!(
        out.lines()
            .next()
            .unwrap()
            .starts_with(&format!("{:<15} FAIL  git is not on PATH", "git")),
        "{out}"
    );
    let home = tempfile::tempdir().unwrap();
    let (code, out, _) = run_with_env(d.path(), &good_env(home.path()), &["doctor"], "");
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("{:<15} ok    not needed\n", "trust")),
        "{out}"
    );
}

/// The editor walk of 2026-11-27: `spellbook rules` is longer than the other names, and its
/// row broke the column. Every row's `ok` or `FAIL` starts in the same column.
#[test]
fn doctor_rows_line_up_whatever_the_longest_name() {
    let d = git_project();
    let home = tempfile::tempdir().unwrap();
    let (_, out, _) = run_with_env(d.path(), &good_env(home.path()), &["doctor"], "");
    let columns: std::collections::BTreeSet<usize> = out
        .lines()
        .map(|line| {
            line.find(" ok    ")
                .or_else(|| line.find(" FAIL  "))
                .unwrap_or_else(|| panic!("no status in {line:?}"))
        })
        .collect();
    assert_eq!(columns.len(), 1, "{out}");
}

fn at(day_time: &str, verb: &str, cost: f64) -> sorcerer_core::domain::errand::Errand {
    let mut e = errand(verb, "haiku", cost);
    e.at = day_time.parse().unwrap();
    e
}

#[test]
fn cost_counts_a_day_of_changes_and_try_harder() {
    let d = git_project();
    let project = Project::open(d.path()).unwrap();
    let ledger = JsonlLedger::for_project(&project).unwrap();
    for e in [
        at("2026-11-21T10:00:00Z", "conjure", 0.001),
        at("2026-11-22T10:00:00Z", "conjure", 0.002),
        at("2026-11-22T11:00:00Z", "conjure", 0.003),
        at("2026-11-22T11:30:00Z", "try-harder", 0.02),
        at("2026-11-22T11:40:00Z", "conjure-retry", 0.005),
    ] {
        ledger.append(&e).unwrap();
    }
    let mut other = at("2026-11-22T11:30:00Z", "review", 5.0);
    other.product = "scry".into();
    ledger.append(&other).unwrap();
    let (code, out, err) = run(d.path(), &["cost"], "");
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "day         changes  try harder  retries  cost\n2026-11-21  1        0           0        $0.001\n2026-11-22  2        1           1        $0.030\n"
    );
    let (_, out, _) = run(d.path(), &["cost", "--since", "2026-11-22"], "");
    assert!(!out.contains("2026-11-21"), "{out}");
    let (_, out, _) = run(d.path(), &["cost", "--json"], "");
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(json["days"][1]["changes"], 2);
    assert_eq!(json["days"][1]["try_harder"], 1);
    assert_eq!(json["days"][1]["retries"], 1);
    let (code, _, err) = run(d.path(), &["cost", "--since", "soon"], "");
    assert_eq!(code, 2);
    assert!(err.contains("YYYY-MM-DD"), "{err}");
}

fn record(id: &str, at: &str, choice: Choice) -> ChangeRecord {
    ChangeRecord {
        version: 1,
        id: id.into(),
        at: at.parse().unwrap(),
        file: "src/a.py".into(),
        line: 12,
        unit: "A.f".into(),
        trigger: Trigger::Cursor,
        attempt: Attempt::First,
        try_harder_of: None,
        facts: RecordedFacts {
            parse_errors: vec![],
            todo_lines: vec![],
            body_empty: true,
        },
        model: "haiku".into(),
        goal: Some(format!("goal {id}")),
        files: vec![RecordedFile {
            path: "src/a.py".into(),
            new: false,
            lines_changed: 3,
            parse_errors: vec![],
        }],
        dropped: vec![],
        seconds: 4.0,
        cost_usd: Some(0.001),
        choice,
    }
}

#[test]
fn log_prints_the_last_records_newest_first() {
    let d = git_project();
    let project = Project::open(d.path()).unwrap();
    let store =
        conjure::adapters::jsonl_change_store::JsonlChangeStore::for_project(&project).unwrap();
    for (i, choice) in [Choice::Kept, Choice::Undone, Choice::NoChange]
        .into_iter()
        .enumerate()
    {
        store
            .append(&record(
                &format!("id{i}"),
                &format!("2026-11-22T1{i}:00:00Z"),
                choice,
            ))
            .unwrap();
    }
    let (code, out, err) = run(d.path(), &["log", "2"], "");
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "2026-11-22 12:00 · src/a.py:12 · A.f · goal id2 · haiku · $0.001 · no-change\n\
         2026-11-22 11:00 · src/a.py:12 · A.f · goal id1 · haiku · $0.001 · undone\n"
    );
    let (_, out, _) = run(d.path(), &["log"], "");
    assert_eq!(out.lines().count(), 3);
    let empty = git_project();
    let (code, _, err) = run(empty.path(), &["log"], "");
    assert_eq!(code, 0);
    assert!(err.contains("no changes recorded yet"), "{err}");
}

#[test]
fn trust_puts_the_root_on_the_list() {
    let d = git_project();
    write(
        d.path(),
        ".sorcerer/config.toml",
        "[model.providers.mine]\nrunner = \"/x/run\"\n",
    );
    let home = tempfile::tempdir().unwrap();
    let env = Env::default().with_user_dirs(home.path());
    let root = Project::open(d.path()).unwrap().root;
    let (code, out, err) = run_with_env(d.path(), &env, &["trust"], "");
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        format!(
            "trusted: {}\n[model.providers.mine] runner = /x/run\n",
            root.display()
        )
    );
    let list = sorcerer_core::adapters::trust_list::TrustList::user(&env).unwrap();
    assert!(list.is_trusted(&root));
    let (_, out, _) = run_with_env(d.path(), &env, &["trust", "--revoke"], "");
    assert_eq!(out, format!("no longer trusted: {}\n", root.display()));
    assert!(!list.is_trusted(&root));
    let (_, out, _) = run_with_env(d.path(), &env, &["trust", "--revoke"], "");
    assert_eq!(out, format!("was not trusted: {}\n", root.display()));
}
