mod support;

use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use scry::cli::{doctor_rows, main_with};
use scry::live::{Live, Wired, checker_over, jev_for};
use scry::pass::{PassDeps, Progress};
use sorcerer_core::env::MapEnv;
use sorcerer_core::fences::Fences;
use sorcerer_core::ledger::Ledger;
use sorcerer_core::view::{CodebaseView, FilesView};
use sorcerer_core::{Cancel, Project};
use sorcerer_models::checker::{Checked, Claim, ClaimKind, Verdict};
use sorcerer_models::decide::{Decider, Question, Shape};
use sorcerer_models::research::{Research, Researched, Researcher};
use support::{FixedClock, StubDecider, StubFinder, commit, git_project, write};
use tempfile::TempDir;

const ORDERS: &str = "def cancel(order):\n    return order.cancel()\n\n\ndef place(order):\n    return order.place()\n";

fn t0() -> DateTime<Utc> {
    "2026-11-18T23:00:00Z".parse().unwrap()
}

/// orders.py committed, then `cancel` changed in the working tree.
fn changed_project() -> TempDir {
    let dir = git_project();
    write(dir.path(), "orders.py", ORDERS);
    commit(dir.path(), "orders");
    write(
        dir.path(),
        "orders.py",
        &ORDERS.replace("order.cancel()", "order.cancel(now=True)"),
    );
    dir
}

/// A find reply with one bug on orders.py line 2, citing line 2.
fn bug_reply(statement: &str) -> String {
    serde_json::json!({
        "findings": [{
            "kind": "bug",
            "file": "orders.py",
            "lines": [2, 2],
            "statement": statement,
            "evidence": [{"file": "orders.py", "lines": [2, 2]}],
        }],
        "open": [],
    })
    .to_string()
}

type CheckFn = dyn Fn(&Claim) -> sorcerer_models::Result<Checked> + Send + Sync;

/// The pass's doubles: the vendored grammars' view, a stub finder and
/// decider, a fixed clock and a claim checker that says right.
struct Doubles {
    view: FilesView,
    finder: StubFinder,
    decider: StubDecider,
    clock: FixedClock,
    checker: Box<CheckFn>,
}

fn quiet(_: &Progress) {}

impl Doubles {
    fn new(dir: &Path, finder: StubFinder) -> Doubles {
        let (_, view) = support::project_with_vendored_grammars(dir);
        Doubles {
            view,
            finder,
            decider: StubDecider::new(),
            clock: FixedClock::at(t0()),
            checker: Box::new(|_: &Claim| {
                Ok(Checked {
                    verdict: Verdict::Right,
                    why: "it does".into(),
                    citations: Vec::new(),
                    escalated: false,
                    errands: Vec::new(),
                })
            }),
        }
    }
}

impl Wired for &Doubles {
    fn deps(&self) -> PassDeps<'_> {
        PassDeps {
            view: &self.view,
            finder: &self.finder,
            decider: &self.decider,
            checker: &*self.checker,
            clock: &self.clock,
            progress: &quiet,
        }
    }
}

struct Ran {
    code: i32,
    out: String,
    err: String,
}

fn args(a: &[&str]) -> Vec<OsString> {
    std::iter::once("scry")
        .chain(a.iter().copied())
        .map(OsString::from)
        .collect()
}

/// `scry ARGS` in DIR with the doubles D.
fn scry(dir: &Path, d: &Doubles, a: &[&str]) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let env = MapEnv::default();
    let code = main_with(
        args(a),
        dir,
        &env,
        &mut out,
        &mut err,
        100,
        |_: &Project| Ok(d),
    );
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn a_clean_tree_prints_nothing_and_exits_0() {
    let dir = git_project();
    write(dir.path(), "orders.py", ORDERS);
    commit(dir.path(), "orders");
    let d = Doubles::new(dir.path(), StubFinder::new());
    let r = scry(dir.path(), &d, &[]);
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "", ""));
    assert!(d.finder.asks().is_empty());
}

#[test]
fn an_open_bug_prints_the_finding_and_exits_1() {
    let dir = changed_project();
    let d = Doubles::new(
        dir.path(),
        StubFinder::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let r = scry(dir.path(), &d, &[]);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    let lines: Vec<&str> = r.out.lines().collect();
    assert_eq!(lines.len(), 4, "{}", r.out);
    assert_eq!(
        lines[0],
        "orders.py:2  bug  cancel() passes now=True  (1.00)"
    );
    assert_eq!(lines[1], "  evidence: orders.py:2");
    let id = lines[2].strip_prefix("  scry why ").expect(lines[2]);
    assert_eq!(id.len(), 6);
    assert_eq!(lines[3], "1 finding, 1 unit looked at, $0.04");
    assert_eq!(r.err, "");
}

#[test]
fn a_long_statement_wraps_under_itself() {
    let dir = changed_project();
    let long = "cancel() returns None when the repository has no order with that id; the caller in api/orders.py:88 indexes the result.";
    let d = Doubles::new(
        dir.path(),
        StubFinder::new().reply(&["orders.py"], &bug_reply(long)),
    );
    let r = scry(dir.path(), &d, &[]);
    let lines: Vec<&str> = r.out.lines().collect();
    assert!(lines[0].starts_with("orders.py:2  bug  cancel() returns None"));
    assert!(lines[0].len() <= 100, "{}", lines[0]);
    assert!(lines[1].starts_with(&" ".repeat(18)), "{}", r.out);
    assert!(lines[1].trim_start().len() > 3);
    assert!(lines[1].ends_with("(1.00)"), "{}", r.out);
}

#[test]
fn a_second_run_prints_the_stored_findings_again_and_exits_1() {
    let dir = changed_project();
    let d = Doubles::new(
        dir.path(),
        StubFinder::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let first = scry(dir.path(), &d, &[]);
    let second = scry(dir.path(), &d, &[]);
    assert_eq!((first.code, second.code), (1, 1));
    assert_eq!(d.finder.asks().len(), 1);
    let finding = |r: &Ran| r.out.lines().take(3).collect::<Vec<_>>().join("\n");
    assert_eq!(finding(&first), finding(&second));
    assert!(
        second
            .out
            .ends_with("1 finding, 0 units looked at, $0.00\n"),
        "{}",
        second.out
    );
}

#[test]
fn json_is_one_object_a_line() {
    let dir = changed_project();
    let d = Doubles::new(
        dir.path(),
        StubFinder::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let r = scry(dir.path(), &d, &["--json"]);
    assert_eq!(r.code, 1);
    let lines: Vec<&str> = r.out.lines().collect();
    assert_eq!(lines.len(), 1, "{}", r.out);
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    for k in [
        "id",
        "kind",
        "rule",
        "statement",
        "file",
        "lines",
        "unit",
        "evidence",
        "keep",
        "verdict",
        "status",
    ] {
        assert!(v.get(k).is_some(), "{k} missing: {v}");
    }
    assert_eq!(v["kind"], "bug");
    assert_eq!(v["file"], "orders.py");
    assert_eq!(v["lines"], serde_json::json!([2, 2]));
    assert_eq!(v["unit"], "cancel");
    assert_eq!(
        v["evidence"],
        serde_json::json!([{"file": "orders.py", "lines": [2, 2]}])
    );
    assert_eq!(v["verdict"], "right");
    assert_eq!(v["status"], "open");
    assert_eq!(r.err, "1 finding, 1 unit looked at, $0.04\n");
}

#[test]
fn a_failed_find_with_nothing_open_exits_2() {
    let dir = changed_project();
    let d = Doubles::new(
        dir.path(),
        StubFinder::new().reply(&["orders.py"], "I could not decide."),
    );
    let r = scry(dir.path(), &d, &[]);
    assert_eq!(r.code, 2, "{}{}", r.out, r.err);
    assert!(
        r.err.contains("scry: find failed for orders.py"),
        "{}",
        r.err
    );
}

#[test]
fn named_files_are_taken_from_the_current_directory() {
    let dir = changed_project();
    write(dir.path(), "sub/keep.txt", "");
    let d = Doubles::new(
        dir.path(),
        StubFinder::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let r = scry(&dir.path().join("sub"), &d, &["../orders.py"]);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    assert!(r.out.starts_with("orders.py:2  bug"), "{}", r.out);
}

#[test]
fn since_with_files_is_refused() {
    let dir = changed_project();
    let d = Doubles::new(dir.path(), StubFinder::new());
    let r = scry(dir.path(), &d, &["--since", "main", "orders.py"]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("--since"), "{}", r.err);
    assert!(d.finder.asks().is_empty());
}

#[test]
fn a_bad_since_rev_exits_2_naming_it() {
    let dir = changed_project();
    let d = Doubles::new(dir.path(), StubFinder::new());
    let r = scry(dir.path(), &d, &["--since", "no-such-branch"]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("no-such-branch"), "{}", r.err);
}

#[test]
fn an_unreadable_store_exits_2_untouched() {
    let dir = changed_project();
    let bytes = br#"{"version": 2, "data": {}}"#;
    write(
        dir.path(),
        ".sorcerer/state/scry/findings.json",
        std::str::from_utf8(bytes).unwrap(),
    );
    let d = Doubles::new(
        dir.path(),
        StubFinder::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let r = scry(dir.path(), &d, &[]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("findings.json"), "{}", r.err);
    let path = dir.path().join(".sorcerer/state/scry/findings.json");
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn no_jev_key_exits_2_naming_jev_api_key() {
    let dir = changed_project();
    let env = MapEnv::default().on_path("claude");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_with(args(&[]), dir.path(), &env, &mut out, &mut err, 100, |p| {
        Live::open(p, &env)
    });
    let err = String::from_utf8(err).unwrap();
    assert_eq!(code, 2);
    assert!(err.contains("JEV_API_KEY"), "{err}");
    assert!(out.is_empty());
}

#[test]
fn live_opens_with_claude_and_a_jev_key() {
    let dir = changed_project();
    let env = MapEnv::default()
        .on_path("claude")
        .var_set("JEV_API_KEY", "k");
    let p = Project::open(dir.path()).unwrap();
    assert!(Live::open(&p, &env).is_ok());
    let none = MapEnv::default().var_set("JEV_API_KEY", "k");
    let e = Live::open(&p, &none).err().unwrap().to_string();
    assert!(e.contains("ANTHROPIC_API_KEY"), "{e}");
}

fn doctor(dir: &Path, env: &MapEnv) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_with(
        args(&["doctor"]),
        dir,
        env,
        &mut out,
        &mut err,
        100,
        |_: &Project| -> anyhow::Result<Live> { panic!("doctor wires no pass") },
    );
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn doctor_lists_each_row() {
    let dir = git_project();
    let names = [
        "provider",
        "jev",
        "languages",
        "steering",
        "store",
        "ledger",
    ];
    let good = MapEnv::default()
        .on_path("claude")
        .var_set("JEV_API_KEY", "k");
    let r = doctor(dir.path(), &good);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    let lines: Vec<&str> = r.out.lines().collect();
    assert_eq!(lines.len(), names.len(), "{}", r.out);
    for (l, n) in lines.iter().zip(names) {
        assert!(l.starts_with(n), "{l}");
        assert!(l.contains(" ok "), "{l}");
    }
    assert!(lines[0].contains("claude"), "{}", lines[0]);

    let r = doctor(dir.path(), &MapEnv::default());
    assert_eq!(r.code, 2);
    let row = |n: &str| {
        r.out
            .lines()
            .find(|l| l.starts_with(n))
            .unwrap()
            .to_string()
    };
    let provider = row("provider");
    assert!(provider.contains("FAIL"), "{provider}");
    assert!(provider.contains("claude on PATH"), "{provider}");
    assert!(provider.contains("ANTHROPIC_API_KEY"), "{provider}");
    assert!(row("jev").contains("JEV_API_KEY"), "{}", r.out);
    // doctor writes nothing
    assert!(!dir.path().join(".sorcerer").exists());
}

#[test]
fn doctor_rows_load_the_project_languages_and_name_a_bad_store() {
    let dir = changed_project();
    write(
        dir.path(),
        ".sorcerer/state/scry/findings.json",
        r#"{"version": 2, "data": {}}"#,
    );
    let (project, _) = support::project_with_vendored_grammars(dir.path());
    let env = MapEnv::default()
        .on_path("claude")
        .var_set("JEV_API_KEY", "k");
    let rows = doctor_rows(&project, &env, &support::test_cache());
    let get = |n: &str| rows.iter().find(|(m, _)| m == n).unwrap().1.clone();
    assert_eq!(get("languages"), Ok("python load".to_string()));
    let store = get("store").unwrap_err();
    assert!(store.contains("findings.json"), "{store}");
    assert!(get("ledger").is_ok());
}

/// A researcher that records what it was asked as, and fails.
struct Recording(Mutex<Vec<(String, String)>>);

impl Researcher for Recording {
    fn research(&self, r: Research, _: &Cancel) -> sorcerer_models::Result<Researched> {
        self.0.lock().unwrap().push((r.product, r.verb));
        Err(sorcerer_models::Error::Reply {
            provider: "stub".into(),
            why: "no reply in tests".into(),
        })
    }
}

#[test]
fn the_live_claim_check_and_jev_record_as_scry() {
    let dir = changed_project();
    let (project, view) = support::project_with_vendored_grammars(dir.path());
    let researcher = Arc::new(Recording(Mutex::new(Vec::new())));
    let check = checker_over(
        researcher.clone(),
        Arc::new(StubDecider::new()),
        Fences::for_project(&project).unwrap(),
        Arc::new(view) as Arc<dyn CodebaseView>,
        Cancel::default(),
    );
    let claim = Claim {
        statement: "cancel passes now".into(),
        about: Vec::new(),
        kind: ClaimKind::Code,
    };
    let _ = check(&claim);
    let asked = researcher.0.lock().unwrap().clone();
    assert!(!asked.is_empty());
    assert!(
        asked.iter().all(|(p, v)| p == "scry" && v == "check"),
        "{asked:?}"
    );

    // Jev, refused at a closed local port, still writes its errand
    let env = MapEnv::default().var_set("JEV_API_KEY", "k");
    let jev = jev_for(&project, &env)
        .unwrap()
        .with_url("http://127.0.0.1:9")
        .with_backoff(Duration::ZERO);
    let q = Question {
        kind: "nitpick".into(),
        instructions: "Is it?".into(),
        shape: Shape::Noul {
            yes: "yes".into(),
            no: "no".into(),
        },
        context: Vec::new(),
        unsure_allowed: false,
    };
    let _ = jev.ask(&[q], &Cancel::default());
    let rows = Ledger::for_project(&project).unwrap().read().unwrap();
    assert!(!rows.errands.is_empty());
    assert!(
        rows.errands
            .iter()
            .all(|e| e.product == "scry" && e.verb == "decide"),
        "{:?}",
        rows.errands
    );
}
