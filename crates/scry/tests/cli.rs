#![allow(clippy::too_many_lines)]
mod support;

use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use scry::adapters::wiring::{LiveWiring, claim_checker, jev_for};
use scry::cli::{doctor_rows, main_with};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::domain::claim::{Checked, Claim, ClaimKind, Escalation, Verdict};
use sorcerer_models::domain::decide::{Question, Shape, UnsureAnswer};
use sorcerer_models::domain::research::{Research, Researched};
use sorcerer_models::ports::decider::Decider;
use sorcerer_models::ports::researcher::Researcher;
use support::{Doubles, FixedClock, StubDecider, StubReviewer, commit, git_project, write};
use tempfile::TempDir;

const ORDERS: &str = "def cancel(order):\n    return order.cancel()\n\n\ndef place(order):\n    return order.place()\n";

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

/// A review reply with one bug on orders.py line 2, citing line 2.
fn bug_reply(statement: &str) -> String {
    serde_json::json!({
        "findings": [{
            "issue_type": "bug",
            "file": "orders.py",
            "lines": [2, 2],
            "statement": statement,
            "evidence": [{"file": "orders.py", "lines": [2, 2]}],
        }],
        "open": [],
    })
    .to_string()
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
    let d = Doubles::new(dir.path(), StubReviewer::new());
    let r = scry(dir.path(), &d, &[]);
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "", ""));
    assert!(d.reviewer.asks().is_empty());
}

#[test]
fn an_open_bug_prints_the_finding_and_exits_1() {
    let dir = changed_project();
    let d = Doubles::new(
        dir.path(),
        StubReviewer::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
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
        StubReviewer::new().reply(&["orders.py"], &bug_reply(long)),
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
        StubReviewer::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let first = scry(dir.path(), &d, &[]);
    let second = scry(dir.path(), &d, &[]);
    assert_eq!((first.code, second.code), (1, 1));
    assert_eq!(d.reviewer.asks().len(), 1);
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
        StubReviewer::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let r = scry(dir.path(), &d, &["--json"]);
    assert_eq!(r.code, 1);
    let lines: Vec<&str> = r.out.lines().collect();
    assert_eq!(lines.len(), 1, "{}", r.out);
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    for k in [
        "id",
        "issue_type",
        "rule",
        "statement",
        "file",
        "lines",
        "unit",
        "evidence",
        "confidence",
        "verdict",
        "status",
    ] {
        assert!(v.get(k).is_some(), "{k} missing: {v}");
    }
    assert!(v.get("kind").is_none() && v.get("keep").is_none(), "{v}");
    assert_eq!(v["issue_type"], "bug");
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
fn scry_off_exits_0_without_a_review() {
    let dir = changed_project();
    write(
        dir.path(),
        ".sorcerer/config.toml",
        "[scry]\nenabled = false\n",
    );
    let d = Doubles::new(
        dir.path(),
        StubReviewer::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let r = scry(dir.path(), &d, &[]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert_eq!(r.out, "");
    assert!(r.err.contains("enabled = false"), "{}", r.err);
    assert!(d.reviewer.asks().is_empty());
}

#[test]
fn scry_off_exits_0_before_any_wiring() {
    let dir = changed_project();
    write(
        dir.path(),
        ".sorcerer/config.toml",
        "[scry]\nenabled = false\n",
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_with(
        args(&[]),
        dir.path(),
        &MapEnv::default(),
        &mut out,
        &mut err,
        100,
        |_: &Project| -> anyhow::Result<&Doubles> { anyhow::bail!("no JEV_API_KEY") },
    );
    let err = String::from_utf8(err).unwrap();
    assert_eq!(code, 0, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("enabled = false"), "{err}");
}

#[test]
fn a_failed_review_with_nothing_open_exits_2() {
    let dir = changed_project();
    let d = Doubles::new(
        dir.path(),
        StubReviewer::new().reply(&["orders.py"], "I could not decide."),
    );
    let r = scry(dir.path(), &d, &[]);
    assert_eq!(r.code, 2, "{}{}", r.out, r.err);
    assert!(
        r.err.contains("scry: review failed for orders.py"),
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
        StubReviewer::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let r = scry(&dir.path().join("sub"), &d, &["../orders.py"]);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    assert!(r.out.starts_with("orders.py:2  bug"), "{}", r.out);
}

#[test]
fn since_with_files_is_refused() {
    let dir = changed_project();
    let d = Doubles::new(dir.path(), StubReviewer::new());
    let r = scry(dir.path(), &d, &["--since", "main", "orders.py"]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("--since"), "{}", r.err);
    assert!(d.reviewer.asks().is_empty());
}

#[test]
fn a_bad_since_rev_exits_2_naming_it() {
    let dir = changed_project();
    let d = Doubles::new(dir.path(), StubReviewer::new());
    let r = scry(dir.path(), &d, &["--since", "no-such-branch"]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("no-such-branch"), "{}", r.err);
}

#[test]
fn an_unreadable_store_exits_2_untouched() {
    let dir = changed_project();
    let bytes = br#"{"version": 3, "data": {}}"#;
    write(
        dir.path(),
        ".sorcerer/state/scry/findings.json",
        std::str::from_utf8(bytes).unwrap(),
    );
    let d = Doubles::new(
        dir.path(),
        StubReviewer::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
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
        LiveWiring::open(p, &env)
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
    assert!(LiveWiring::open(&p, &env).is_ok());
    let none = MapEnv::default().var_set("JEV_API_KEY", "k");
    let e = LiveWiring::open(&p, &none).err().unwrap().to_string();
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
        |_: &Project| -> anyhow::Result<LiveWiring> { panic!("doctor wires no pass") },
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
fn an_old_store_exits_2_naming_its_version() {
    let dir = changed_project();
    let bytes = r#"{"version": 1, "data": {"findings": [], "inspected": {}, "capped": {}}}"#;
    write(dir.path(), ".sorcerer/state/scry/findings.json", bytes);
    let d = Doubles::new(
        dir.path(),
        StubReviewer::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let r = scry(dir.path(), &d, &[]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("findings.json"), "{}", r.err);
    assert!(
        r.err
            .contains("version 1, and this sorcerer reads version 2"),
        "{}",
        r.err
    );
    let path = dir.path().join(".sorcerer/state/scry/findings.json");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    let (project, _) = support::project_with_vendored_grammars(dir.path());
    let env = MapEnv::default()
        .on_path("claude")
        .var_set("JEV_API_KEY", "k");
    let rows = doctor_rows(&project, &env, &support::test_cache());
    let store = rows
        .iter()
        .find(|row| row.name == "store")
        .unwrap()
        .result
        .clone();
    assert!(store.unwrap_err().contains("version 1"));
}

#[test]
fn doctor_rows_load_the_project_languages_and_name_a_bad_store() {
    let dir = changed_project();
    write(
        dir.path(),
        ".sorcerer/state/scry/findings.json",
        r#"{"version": 3, "data": {}}"#,
    );
    let (project, _) = support::project_with_vendored_grammars(dir.path());
    let env = MapEnv::default()
        .on_path("claude")
        .var_set("JEV_API_KEY", "k");
    let rows = doctor_rows(&project, &env, &support::test_cache());
    let get = |n: &str| {
        rows.iter()
            .find(|row| row.name == n)
            .unwrap()
            .result
            .clone()
    };
    assert_eq!(get("languages"), Ok("python load".to_string()));
    let store = get("store").unwrap_err();
    assert!(store.contains("findings.json"), "{store}");
    assert!(get("ledger").is_ok());
}

/// A researcher that records what it was asked as, and the prompt, and fails.
struct Recording(Mutex<Vec<(String, String)>>, Mutex<Vec<String>>);

impl Researcher for Recording {
    fn research(&self, r: Research, _: &Cancel) -> sorcerer_models::Result<Researched> {
        self.1.lock().unwrap().push(r.prompt.clone());
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
    let researcher = Arc::new(Recording(Mutex::new(Vec::new()), Mutex::new(Vec::new())));
    let check = claim_checker(
        researcher.clone(),
        Arc::new(StubDecider::new()),
        Fences::for_project(&project).unwrap(),
        Arc::new(view) as Arc<dyn CodebaseView>,
        project.languages.clone(),
        Cancel::default(),
    );
    let claim = Claim {
        statement: "cancel passes now".into(),
        about: vec![sorcerer_models::domain::claim::Citation {
            file: "orders.py".into(),
            lines: sorcerer_core::domain::unit::Span { start: 1, end: 1 },
            print: None,
        }],
        kind: ClaimKind::Code,
    };
    let _ = check.check(&claim);
    let asked = researcher.0.lock().unwrap().clone();
    assert!(!asked.is_empty());
    // the project's languages reach the prompt: a Python file's unit wording
    let prompt = researcher.1.lock().unwrap()[0].clone();
    assert!(prompt.contains("(Class.method)"), "{prompt}");
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
        unsure: UnsureAnswer::NotAllowed,
    };
    let _ = jev.ask(&[q], &Cancel::default());
    let rows = JsonlLedger::for_project(&project).unwrap().read().unwrap();
    assert!(!rows.errands.is_empty());
    assert!(
        rows.errands
            .iter()
            .all(|e| e.product == "scry" && e.verb == "decide"),
        "{:?}",
        rows.errands
    );
}

// ---- list, cost, dismiss, why ----

mod stored {
    use super::*;
    use chrono::FixedOffset;
    use scry::domain::finding::{
        Evidence, Finding, FindingId, Place, Scores, Status, StoredVerdict,
    };
    use scry::domain::findings::Findings;
    use sorcerer_core::domain::unit::Span;

    pub fn finding(id: &str, issue_type: &str, status: Status, file: &str, line: u32) -> Finding {
        let t: DateTime<Utc> = "2026-11-18T12:00:00Z".parse().unwrap();
        Finding {
            id: FindingId(id.into()),
            issue_type: issue_type.into(),
            rule: None,
            statement: format!("statement of {id}"),
            place: Place {
                file: file.into(),
                unit: Some("cancel".into()),
                lines: Span {
                    start: line,
                    end: line + 1,
                },
                unit_start: Some(1),
                fingerprint: None,
            },
            evidence: vec![Evidence {
                file: file.into(),
                lines: Span {
                    start: line,
                    end: line,
                },
                print: None,
            }],
            scores: Scores {
                confidence: Some(0.9),
                asked_on: None,
            },
            verdict: Some(StoredVerdict {
                verdict: "right".into(),
                why: "the call passes it".into(),
                cited: Vec::new(),
            }),
            status,
            seen: 3,
            first: t,
            last: t,
            history: vec![(t, Status::Open)],
            model: "sonnet".into(),
            cost_usd: 0.04,
        }
    }

    pub fn put(dir: &Path, findings: Vec<Finding>) {
        let p = Project::open(dir).unwrap();
        support::update_store(&p, |s| {
            *s = Findings {
                findings,
                ..Default::default()
            }
        })
        .unwrap();
    }

    pub fn three(dir: &Path) {
        let mut long = finding("aaaa02", "bug", Status::Open, "orders.py", 2);
        long.rule = Some("result".into());
        long.statement = "x".repeat(100);
        put(
            dir,
            vec![
                finding("cccc03", "convention", Status::Withheld, "a.py", 9),
                long,
                finding("bbbb01", "assumption", Status::Open, "orders.py", 1),
            ],
        );
    }

    pub fn ny() -> FixedOffset {
        FixedOffset::west_opt(5 * 3600).unwrap()
    }
}

fn plain_doubles(dir: &Path) -> Doubles {
    Doubles::new(dir, StubReviewer::new())
}

#[test]
fn list_prints_the_table_open_first() {
    let dir = changed_project();
    stored::three(dir.path());
    let d = plain_doubles(dir.path());
    let r = scry(dir.path(), &d, &["list"]);
    let cut = format!("{}…", "x".repeat(79));
    let want = format!(
        "status    issue type  file       line  rule    statement\n\
         open      assumption  orders.py  1     -       statement of bbbb01\n\
         open      bug         orders.py  2     result  {cut}\n\
         withheld  convention  a.py       9     -       statement of cccc03\n"
    );
    assert_eq!(
        (r.code, r.out.as_str(), r.err.as_str()),
        (0, want.as_str(), "")
    );
}

#[test]
fn list_filters_by_status() {
    let dir = changed_project();
    stored::three(dir.path());
    let d = plain_doubles(dir.path());
    let r = scry(dir.path(), &d, &["list", "--status", "withheld"]);
    assert_eq!(r.code, 0);
    assert_eq!(r.out.lines().count(), 2, "{}", r.out);
    assert!(r.out.contains("cccc03") && !r.out.contains("bbbb01"));
    let r = scry(
        dir.path(),
        &d,
        &["list", "--status", "withheld", "--status", "open"],
    );
    assert_eq!(r.out.lines().count(), 4);
}

#[test]
fn list_json_has_the_fields() {
    let dir = changed_project();
    stored::three(dir.path());
    let d = plain_doubles(dir.path());
    let r = scry(dir.path(), &d, &["list", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    let f = &v["findings"][1];
    assert_eq!(f["id"], "aaaa02");
    assert_eq!(f["status"], "open");
    assert_eq!(f["issue_type"], "bug");
    assert_eq!(f["file"], "orders.py");
    assert_eq!(f["line"], 2);
    assert_eq!(f["rule"], "result");
    assert_eq!(f["confidence"], 0.9);
    assert_eq!(f["verdict"], "right");
    assert_eq!(f["seen"], 3);
    assert_eq!(v["findings"][0]["rule"], serde_json::Value::Null);
}

fn ledger_at(dir: &Path, rows: &[(&str, &str, &str, Option<f64>)]) {
    let p = Project::open(dir).unwrap();
    let ledger = JsonlLedger::for_project(&p).unwrap();
    for (at, product, verb, cost) in rows {
        let mut e = support::errand(verb, "sonnet", 0.0);
        e.at = at.parse().unwrap();
        e.product = (*product).into();
        e.cost_usd = *cost;
        ledger.append(&e).unwrap();
    }
}

fn ledger_fixture(dir: &Path) {
    ledger_at(
        dir,
        &[
            // the 17th in New York, the last two after midnight in UTC
            ("2026-11-17T15:00:00Z", "scry", "review", Some(0.04)),
            ("2026-11-18T03:00:00Z", "scry", "check", Some(0.016)),
            ("2026-11-18T04:00:00Z", "scry", "decide", Some(0.001)),
            // the 18th in New York (it turns at 05:00Z)
            ("2026-11-18T05:30:00Z", "scry", "review", Some(0.04)),
            ("2026-11-18T14:00:00Z", "scry", "review", Some(0.05)),
            ("2026-11-18T14:05:00Z", "scry", "check", Some(0.02)),
            ("2026-11-18T14:06:00Z", "scry", "decide", Some(0.01)),
            ("2026-11-18T14:07:00Z", "conjure", "review", Some(9.0)),
        ],
    );
}

fn ny_doubles(dir: &Path) -> Doubles {
    let mut d = plain_doubles(dir);
    d.clock = FixedClock::at("2026-11-18T20:00:00Z".parse().unwrap()).in_zone(stored::ny());
    d
}

#[test]
fn cost_sums_by_local_day() {
    let dir = changed_project();
    ledger_fixture(dir.path());
    let mut s = stored::finding(
        "aaaa01",
        "bug",
        scry::domain::finding::Status::Unverified,
        "a.py",
        1,
    );
    s.statement.clear();
    let p = Project::open(dir.path()).unwrap();
    support::update_store(&p, |st| {
        st.findings.push(s);
        st.capped
            .insert("a.py".into(), ["f".to_string(), "g".to_string()].into());
        st.capped.insert("b.py".into(), ["h".to_string()].into());
    })
    .unwrap();
    let d = ny_doubles(dir.path());
    let r = scry(dir.path(), &d, &["cost"]);
    let want = "day         reviews  checks  review $  check $  total $\n\
                2026-11-17  1        1       0.04      0.02     0.06\n\
                2026-11-18  2        1       0.09      0.02     0.12\n\
                waiting on the cap: 3 units\n";
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, want, ""));
    let j = scry(dir.path(), &d, &["cost", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&j.out).unwrap();
    assert_eq!(v["waiting_units"], 3);
    assert_eq!(v["days"][1]["day"], "2026-11-18");
    assert_eq!(v["days"][1]["reviews"], 2);
    assert!(v["days"][1].get("review_cost").is_some(), "{v}");
    assert_eq!(v["days"][0]["checks"], 1);
}

#[test]
fn cost_since_leaves_earlier_days_out() {
    let dir = changed_project();
    ledger_fixture(dir.path());
    let d = ny_doubles(dir.path());
    let r = scry(dir.path(), &d, &["cost", "--since", "2026-11-18"]);
    assert_eq!(
        r.out,
        "day         reviews  checks  review $  check $  total $\n\
         2026-11-18  2        1       0.09      0.02     0.12\n\
         waiting on the cap: 0 units\n"
    );
    let bad = scry(dir.path(), &d, &["cost", "--since", "yesterday"]);
    assert_eq!(bad.code, 2);
}

#[test]
fn cost_names_an_unreadable_store() {
    let dir = changed_project();
    ledger_fixture(dir.path());
    write(
        dir.path(),
        ".sorcerer/state/scry/findings.json",
        r#"{"version": 3, "data": {}}"#,
    );
    let d = ny_doubles(dir.path());
    let r = scry(dir.path(), &d, &["cost"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(!r.out.contains("waiting on the cap"), "{}", r.out);
    assert!(r.err.contains("cannot be read"), "{}", r.err);
    let j = scry(dir.path(), &d, &["cost", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&j.out).unwrap();
    assert!(v["waiting_units"].is_null(), "{v}");
    assert!(j.err.contains("cannot be read"), "{}", j.err);
}

#[test]
fn dismiss_by_prefix_then_a_run_no_longer_prints_it() {
    let dir = changed_project();
    let d = Doubles::new(
        dir.path(),
        StubReviewer::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    );
    let first = scry(dir.path(), &d, &["--json"]);
    assert_eq!(first.code, 1);
    let id = serde_json::from_str::<serde_json::Value>(first.out.lines().next().unwrap()).unwrap()
        ["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = scry(dir.path(), &d, &["dismiss", &id[..4]]);
    assert_eq!(
        (r.code, r.out.as_str()),
        (0, format!("dismissed {id} (here)\n").as_str())
    );
    let again = scry(dir.path(), &d, &[]);
    assert_eq!((again.code, again.out.as_str()), (0, ""));
    let e = scry(dir.path(), &d, &["dismiss", &id, "--everywhere"]);
    assert_eq!(e.out, format!("dismissed {id} (everywhere)\n"));
}

#[test]
fn dismiss_unknown_exits_2() {
    let dir = changed_project();
    stored::three(dir.path());
    let d = plain_doubles(dir.path());
    let r = scry(dir.path(), &d, &["dismiss", "ffffff"]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("no finding ffffff"), "{}", r.err);
    let r = scry(dir.path(), &d, &["why", "ffffff"]);
    assert_eq!(r.code, 2);
}

/// A cited range prints whole, two lines either side, however long (the
/// demo's 24-39 citation printed 16-32, the claim prompt's window).
#[test]
fn why_prints_the_whole_cited_range() {
    let dir = changed_project();
    let mut f = stored::finding(
        "aaaa02",
        "bug",
        scry::domain::finding::Status::Open,
        "orders.py",
        2,
    );
    f.evidence[0].lines = sorcerer_core::domain::unit::Span { start: 5, end: 6 };
    stored::put(dir.path(), vec![f]);
    let d = plain_doubles(dir.path());
    let r = scry(dir.path(), &d, &["why", "aaaa"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let want = "Evidence:\norders.py:5-6\n   3 \n   4 \n   5 def place(order):\n   6     return order.place()\n\n";
    assert!(r.out.contains(want), "{}", r.out);
}

#[test]
fn why_does_not_print_a_fenced_or_outside_citation() {
    let dir = changed_project();
    write(dir.path(), ".env", "SECRET=hunter2\n");
    let mut f = stored::finding(
        "aaaa03",
        "bug",
        scry::domain::finding::Status::Unverified,
        "orders.py",
        2,
    );
    let mut outside = f.evidence[0].clone();
    outside.file = "../outside.txt".into();
    f.evidence[0].file = ".env".into();
    f.evidence[0].lines = sorcerer_core::domain::unit::Span { start: 1, end: 1 };
    f.evidence.push(outside);
    stored::put(dir.path(), vec![f]);
    let d = plain_doubles(dir.path());
    let r = scry(dir.path(), &d, &["why", "aaaa03"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(!r.out.contains("hunter2"), "{}", r.out);
    assert_eq!(
        r.out
            .matches("(not shown: fenced or outside the project)")
            .count(),
        2,
        "{}",
        r.out
    );
}

#[test]
fn why_shows_rule_evidence_verdict_and_history() {
    let dir = changed_project();
    write(
        dir.path(),
        ".sorcerer/scry.toml",
        "[[rule]]\nid = \"result\"\ntext = \"\"\"\nCommands return a Result (must)\nA command returns Result, never panics.\"\"\"\n",
    );
    stored::three(dir.path());
    let d = plain_doubles(dir.path());
    let r = scry(dir.path(), &d, &["why", "aaaa"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let want = format!(
        "# bug: orders.py:2-3\n\n{}\n\n\
         Rule: result\n\
         Commands return a Result (must)\n\
         A command returns Result, never panics.\n\n\
         Unit: cancel (starts at orders.py:1)\n\n\
         Evidence:\n\
         orders.py:2\n\
         \x20  1 def cancel(order):\n\
         \x20  2     return order.cancel(now=True)\n\
         \x20  3 \n\
         \x20  4 \n\n\
         Checked: right: the call passes it\n\
         Confidence: 0.90\n\
         Seen: 3 times since 2026-11-18\n\n\
         History:\n\
         2026-11-18T12:00:00Z  open\n",
        "x".repeat(100)
    );
    assert_eq!(r.out, want);
}

// ---- the run log, -v and scry log ----

fn runs_dir(dir: &Path) -> std::path::PathBuf {
    dir.join(".sorcerer/state/scry/runs")
}

fn run_logs(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(runs_dir(dir))
        .map(|rd| {
            rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn bug_doubles(dir: &Path) -> Doubles {
    Doubles::new(
        dir,
        StubReviewer::new().reply(&["orders.py"], &bug_reply("cancel() passes now=True")),
    )
}

#[test]
fn a_run_writes_one_log_and_twenty_one_runs_leave_twenty() {
    let dir = changed_project();
    let d = bug_doubles(dir.path());
    let r = scry(dir.path(), &d, &[]);
    assert_eq!(r.code, 1, "{}", r.err);
    let logs = run_logs(dir.path());
    assert_eq!(logs, vec!["2026-11-18T23-00-00Z.md".to_string()]);
    let text = std::fs::read_to_string(runs_dir(dir.path()).join(&logs[0])).unwrap();
    for s in [
        "## Run",
        "## Scope",
        "## Review calls",
        "## Findings",
        "## Summary",
        "cancel() passes now=True",
        "exit 1",
    ] {
        assert!(text.contains(s), "no {s:?} in\n{text}");
    }
    // the store commands write no log
    scry(dir.path(), &d, &["list"]);
    assert_eq!(run_logs(dir.path()).len(), 1);
    for _ in 0..20 {
        scry(dir.path(), &d, &[]);
    }
    let logs = run_logs(dir.path());
    assert_eq!(logs.len(), 20, "{logs:?}");
    // the oldest, the first run's, was deleted
    assert!(!logs.contains(&"2026-11-18T23-00-00Z.md".to_string()));
    assert!(logs.contains(&"2026-11-18T23-00-00Z-2.md".to_string()));
    assert!(logs.contains(&"2026-11-18T23-00-00Z-21.md".to_string()));
}

#[test]
fn scry_log_prints_the_latest_the_one_before_a_list_and_a_path() {
    let dir = changed_project();
    let d = bug_doubles(dir.path());
    scry(dir.path(), &d, &[]);
    scry(dir.path(), &d, &[]);
    let latest = scry(dir.path(), &d, &["log"]);
    assert_eq!(latest.code, 0, "{}", latest.err);
    assert!(
        latest.out.contains("1 finding shown, 0 units looked at"),
        "{}",
        latest.out
    );
    let before = scry(dir.path(), &d, &["log", "2"]);
    assert_eq!(before.code, 0);
    assert!(
        before.out.contains("1 finding shown, 1 unit looked at"),
        "{}",
        before.out
    );
    assert!(before.out.contains("## Review calls"));

    let list = scry(dir.path(), &d, &["log", "--list"]);
    assert_eq!(list.code, 0);
    let lines: Vec<&str> = list.out.lines().collect();
    assert_eq!(lines.len(), 3, "{}", list.out);
    assert!(
        lines[1].starts_with("2026-11-18T23-00-00Z-2.md"),
        "{}",
        list.out
    );
    assert!(lines[1].contains("2026-11-18 23:00"), "{}", list.out);
    assert!(lines[1].contains("0 units looked at"), "{}", list.out);
    assert!(
        lines[2].starts_with("2026-11-18T23-00-00Z.md"),
        "{}",
        list.out
    );

    let path = scry(dir.path(), &d, &["log", "--path"]);
    assert_eq!(path.code, 0);
    let p = std::path::PathBuf::from(path.out.trim_end());
    assert!(
        p.ends_with(".sorcerer/state/scry/runs/2026-11-18T23-00-00Z-2.md"),
        "{p:?}"
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), latest.out);

    let past = scry(dir.path(), &d, &["log", "3"]);
    assert_eq!(past.code, 2);
    assert_eq!(past.err.lines().count(), 1, "{}", past.err);
}

#[test]
fn scry_log_with_no_log_exits_2_on_one_line() {
    let dir = changed_project();
    let d = plain_doubles(dir.path());
    for a in [&["log"][..], &["log", "--list"], &["log", "--path"]] {
        let r = scry(dir.path(), &d, a);
        assert_eq!((r.code, r.out.as_str()), (2, ""), "{a:?}");
        assert_eq!(r.err.lines().count(), 1, "{}", r.err);
        assert!(r.err.starts_with("scry: "), "{}", r.err);
    }
}

#[test]
fn verbose_streams_the_log_on_stderr_and_leaves_stdout_as_it_was() {
    let quiet_dir = changed_project();
    let loud_dir = changed_project();
    let (dq, dl) = (bug_doubles(quiet_dir.path()), bug_doubles(loud_dir.path()));
    let quiet = scry(quiet_dir.path(), &dq, &[]);
    let loud = scry(loud_dir.path(), &dl, &["-v"]);
    assert_eq!(loud.code, quiet.code);
    assert_eq!(loud.out, quiet.out);
    assert_eq!(quiet.err, "");
    let log =
        std::fs::read_to_string(runs_dir(loud_dir.path()).join(&run_logs(loud_dir.path())[0]))
            .unwrap();
    assert_eq!(loud.err, log);
    assert!(loud.err.contains("## Review calls"));

    let json_dir = changed_project();
    let dj = bug_doubles(json_dir.path());
    let j = scry(json_dir.path(), &dj, &["--verbose", "--json"]);
    assert_eq!(j.code, 1);
    for line in j.out.lines() {
        serde_json::from_str::<serde_json::Value>(line).expect(line);
    }
    assert!(j.err.contains("## Review calls"), "{}", j.err);
}

#[test]
fn why_names_the_citation_a_check_could_not_confirm() {
    use sorcerer_models::domain::claim::{CitationCheck, Cited, RawCitation};
    let dir = changed_project();
    let mut d = bug_doubles(dir.path());
    d.checker = Box::new(|_: &Claim| {
        Ok(Checked {
            verdict: Verdict::CannotTell,
            why: "cited code not found".into(),
            citations: Vec::new(),
            cited: vec![
                Cited {
                    raw: RawCitation {
                        path: "orders.py".into(),
                        unit: Some("cancel".into()),
                        line: Some(2),
                    },
                    check: CitationCheck::Confirmed,
                },
                Cited {
                    raw: RawCitation {
                        path: "orders.py".into(),
                        unit: Some("Order.refund".into()),
                        line: None,
                    },
                    check: CitationCheck::NotConfirmed,
                },
            ],
            escalation: Escalation::No,
            errands: Vec::new(),
        })
    });
    let r = scry(dir.path(), &d, &[]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    let list = scry(dir.path(), &d, &["list", "--status", "unsure"]);
    assert_eq!(list.out.lines().count(), 2, "{}", list.out);
    let s = scry(dir.path(), &d, &["list", "--json"]);
    let v: serde_json::Value = serde_json::from_str(s.out.trim()).unwrap();
    let id = v["findings"][0]["id"].as_str().unwrap().to_string();
    let w = scry(dir.path(), &d, &["why", &id]);
    assert_eq!(w.code, 0, "{}", w.err);
    assert!(
        w.out.contains("Checked: cannot_tell: cited code not found"),
        "{}",
        w.out
    );
    assert!(w.out.contains("Cited by the check:\n"), "{}", w.out);
    assert!(
        w.out.contains("orders.py cancel line 2: confirmed\n"),
        "{}",
        w.out
    );
    assert!(
        w.out.contains("orders.py Order.refund: not found\n"),
        "{}",
        w.out
    );
}

#[test]
fn why_and_the_log_say_a_citation_stood_on_its_line() {
    use sorcerer_models::domain::claim::{CitationCheck, Cited, RawCitation};
    let dir = changed_project();
    let mut d = bug_doubles(dir.path());
    d.checker = Box::new(|_: &Claim| {
        Ok(Checked {
            verdict: Verdict::Right,
            why: "it does".into(),
            citations: Vec::new(),
            cited: vec![Cited {
                raw: RawCitation {
                    path: "orders.py".into(),
                    unit: Some("Orders::cancel".into()),
                    line: Some(2),
                },
                check: CitationCheck::ConfirmedOnTheLineOnly,
            }],
            escalation: Escalation::No,
            errands: Vec::new(),
        })
    });
    let r = scry(dir.path(), &d, &["--verbose"]);
    assert!(
        r.err
            .contains("`orders.py` `Orders::cancel` line 2: unit not found, confirmed on line 2"),
        "{}",
        r.err
    );
    let s = scry(dir.path(), &d, &["list", "--json"]);
    let v: serde_json::Value = serde_json::from_str(s.out.trim()).unwrap();
    let id = v["findings"][0]["id"].as_str().unwrap().to_string();
    let w = scry(dir.path(), &d, &["why", &id]);
    assert!(
        w.out
            .contains("orders.py Orders::cancel line 2: unit not found, confirmed on line 2\n"),
        "{}",
        w.out
    );
}

#[test]
fn a_store_written_before_cited_still_reads() {
    let v: scry::domain::finding::StoredVerdict =
        serde_json::from_str(r#"{"verdict": "right", "why": "it does"}"#).unwrap();
    assert!(v.cited.is_empty());
    // and a verdict with nothing cited is written as before
    let back = serde_json::to_string(&v).unwrap();
    assert_eq!(back, r#"{"verdict":"right","why":"it does"}"#);
    let dir = changed_project();
    stored::three(dir.path());
    let d = plain_doubles(dir.path());
    let r = scry(dir.path(), &d, &["why", "aaaa"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(!r.out.contains("Cited by the check"), "{}", r.out);
}

#[test]
fn doctor_says_what_steering_was_understood() {
    let env = MapEnv::default()
        .on_path("claude")
        .var_set("JEV_API_KEY", "k");
    let row = |dir: &Path| {
        let r = doctor(dir, &env);
        r.out
            .lines()
            .find(|l| l.starts_with("steering"))
            .unwrap()
            .to_string()
    };
    let dir = git_project();
    let none = row(dir.path());
    assert!(none.contains(" ok "), "{none}");
    assert!(none.contains("no .sorcerer/scry.toml: no rules"), "{none}");
    write(
        dir.path(),
        ".sorcerer/scry.toml",
        "context = \"Bistro.\"\ninclude = [\"docs/c.md\"]\n\n[[rule]]\nid = \"a\"\ntext = \"t\"\n\n[[rule]]\nid = \"b\"\ntext = \"u\"\n",
    );
    write(dir.path(), "docs/c.md", "Conventions.\n");
    let some = row(dir.path());
    assert!(some.contains("2 rules, 1 include, context 7 B, "), "{some}");
    assert!(some.ends_with("; .sorcerer/scry.toml"), "{some}");
    write(dir.path(), ".sorcerer/scry.md", "## Rules\n");
    let old = row(dir.path());
    assert!(old.contains("FAIL"), "{old}");
    assert!(old.contains("no longer read"), "{old}");
}

#[test]
fn the_run_log_head_says_what_steering_was_understood() {
    let dir = changed_project();
    write(
        dir.path(),
        ".sorcerer/scry.toml",
        "context = \"Orders.\"\n\n[[rule]]\nid = \"result\"\ntext = \"t\"\n\n[[rule]]\nid = \"clock\"\ntext = \"u\"\n",
    );
    let d = bug_doubles(dir.path());
    scry(dir.path(), &d, &[]);
    let logs = run_logs(dir.path());
    let text = std::fs::read_to_string(runs_dir(dir.path()).join(&logs[0])).unwrap();
    let want = "\n## Steering\n\n- file: .sorcerer/scry.toml\n- context: 7 B\n- rules: result, clock\n- size: ";
    assert!(text.contains(want), "{text}");
    assert!(text.contains(" of 32.0 KB\n"), "{text}");
    assert!(text.find("## Steering") < text.find("## Scope"), "{text}");
}

// ---- the command line's texts, pinned before the rewrite ----

/// A unit of N lines named NAME: its definition and N - 1 numbered lines.
fn long_unit(name: &str, n: u32) -> String {
    let mut text = format!("def {name}():\n");
    for i in 1..n {
        text.push_str(&format!("    x{i} = {i}\n"));
    }
    text
}

/// The text of DIR's only run log.
fn the_run_log(dir: &Path) -> String {
    let logs = run_logs(dir);
    assert_eq!(logs.len(), 1, "{logs:?}");
    std::fs::read_to_string(runs_dir(dir).join(&logs[0])).unwrap()
}

/// The run log from its `FIRST` heading to the end of its `LAST` section.
fn sections_of<'l>(log: &'l str, first: &str, last: &str) -> &'l str {
    let start = log
        .find(&format!("\n## {first}\n"))
        .unwrap_or_else(|| panic!("no {first} section in:\n{log}"));
    let at = start
        + log[start..]
            .find(&format!("\n## {last}\n"))
            .unwrap_or_else(|| panic!("no {last} section in:\n{log}"));
    let end = log[at + 1..]
        .find("\n## ")
        .map_or(log.len(), |e| at + 1 + e);
    &log[start..end]
}

/// TEXT equals the golden file NAME; SCRY_BLESS=1 rewrites it.
fn assert_golden(name: &str, text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("SCRY_BLESS").is_some() {
        std::fs::write(&path, text).unwrap();
    }
    let golden = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text, golden,
        "the run log drifted from {name}; SCRY_BLESS=1 rewrites the golden file"
    );
}

/// A review reply of FINDINGS on orders.py, each (issue type, line,
/// statement, cited line).
fn orders_reply(findings: &[(&str, u32, &str, u32)]) -> String {
    let findings: Vec<serde_json::Value> = findings
        .iter()
        .map(|(kind, line, statement, cited)| {
            serde_json::json!({
                "issue_type": kind,
                "file": "orders.py",
                "lines": [line, line],
                "statement": statement,
                "evidence": [{"file": "orders.py", "lines": [cited, cited]}],
            })
        })
        .collect();
    serde_json::json!({"findings": findings, "open": []}).to_string()
}

#[test]
fn the_run_log_summary_is_as_recorded() {
    use scry::domain::finding::Status;
    // orders.py and two files of one long unit each, so three review calls
    let dir = git_project();
    write(dir.path(), "orders.py", ORDERS);
    write(dir.path(), "x_big.py", &long_unit("xray", 600));
    write(dir.path(), "y_big.py", &long_unit("yankee", 600));
    commit(dir.path(), "three files");
    write(
        dir.path(),
        "orders.py",
        &ORDERS.replace("order.cancel()", "order.cancel(now=True)"),
    );
    for file in ["x_big.py", "y_big.py"] {
        let p = dir.path().join(file);
        let text = std::fs::read_to_string(&p).unwrap();
        std::fs::write(p, text.replacen("x1 = 1", "x1 = 2", 1)).unwrap();
    }
    // a convention unverified before the run, in `place` (out of this run's scope)
    let mut before = stored::finding("dddd01", "convention", Status::Unverified, "orders.py", 6);
    before.place.unit = Some("place".into());
    before.place.lines = sorcerer_core::domain::unit::Span { start: 6, end: 6 };
    before.verdict = None;
    // first in the store at the top confidence: it takes round 1's first place
    before.scores.confidence = Some(1.0);
    stored::put(dir.path(), vec![before]);

    let mut reviewer = StubReviewer::new()
        .reply(
            &["orders.py"],
            &orders_reply(&[
                ("convention", 2, "cancel breaks the naming rule", 2),
                ("design", 2, "cancel is hard to test", 2),
                ("bug", 2, "cancel is not idempotent", 2),
                ("bug", 2, "cancel loses the refund", 2),
                ("bug", 2, "cancel forgets the audit", 2),
                ("bug", 2, "cancel cites a missing line", 99),
                ("bug", 2, "cancel has a long name", 2),
                ("bug", 3, "a stray blank line", 3),
            ]),
        )
        .reply(&["x_big.py"], "I could not finish.");
    reviewer.cost = 0.01;
    // each call leaves its ledger row, as the live reviewer's does
    let ledger_dir = dir.path().to_path_buf();
    let reviewer = reviewer.on_review(move |_| {
        let p = Project::open(&ledger_dir).unwrap();
        JsonlLedger::for_project(&p)
            .unwrap()
            .append(&support::errand("review", "sonnet", 0.01))
            .unwrap();
    });
    let mut d = Doubles::new(dir.path(), reviewer);
    d.decider = StubDecider::new().on(
        "nitpick",
        |q| StubDecider::context(q, "finding") == "cancel has a long name",
        StubDecider::p_yes(0.9),
    );
    d.checker = Box::new(|c: &Claim| match c.statement.as_str() {
        "cancel is not idempotent" => Ok(Checked {
            verdict: Verdict::Wrong,
            why: "it checks the state first".into(),
            citations: Vec::new(),
            cited: Vec::new(),
            escalation: Escalation::No,
            errands: vec![support::errand("check", "haiku", 0.01)],
        }),
        "cancel loses the refund" => Ok(Checked {
            verdict: Verdict::CannotTell,
            why: "the refund is not in view".into(),
            citations: Vec::new(),
            cited: Vec::new(),
            escalation: Escalation::ToTheStrongModel,
            errands: vec![support::errand("check", "sonnet", 0.01)],
        }),
        _ => Err(sorcerer_models::Error::Reply {
            provider: "stub".into(),
            why: "no answer".into(),
        }),
    });
    // room for two calls at the $0.04 estimate ($0.01 each spent), not a third
    let r = scry(dir.path(), &d, &["--budget", "0.055"]);
    let log = the_run_log(dir.path());
    assert_eq!(r.code, 1, "{}{}{log}", r.out, r.err);
    assert_golden(
        "run-log-summary.md",
        sections_of(&log, "Summary", "Summary"),
    );
}

#[test]
fn the_run_log_head_is_as_recorded() {
    let dir = changed_project();
    write(dir.path(), "notes.md", "Orders are cancelled by id.\n");
    write(
        dir.path(),
        ".sorcerer/scry.toml",
        "context = \"Orders.\"\ninclude = [\"notes.md\"]\n\n[[rule]]\nid = \"result\"\ntext = \"Commands return a Result.\"\n",
    );
    write(
        dir.path(),
        ".sorcerer/config.toml",
        "[scry]\nbudget_per_day = 2.5\n",
    );
    let d = bug_doubles(dir.path());
    let r = scry(dir.path(), &d, &["--budget", "0.5"]);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    // the machine's: the project's path and scry's version
    let root = dir.path().canonicalize().unwrap();
    let log = the_run_log(dir.path())
        .replace(&root.display().to_string(), "<project>")
        .replace(env!("CARGO_PKG_VERSION"), "<version>");
    let head = &log[..log.find("\n## Scope\n").expect("a Scope section")];
    assert_golden("run-log-head.md", head);

    // a config scry cannot read stops the command before the run: no run
    // log, so the head's own `- config:` line is not reached from here
    let bad = changed_project();
    write(
        bad.path(),
        ".sorcerer/config.toml",
        "[scry]\nissue_types = [\"style\"]\n",
    );
    let d = bug_doubles(bad.path());
    let r = scry(bad.path(), &d, &[]);
    assert_eq!((r.code, r.out.as_str()), (2, ""), "{}", r.err);
    assert!(
        r.err.ends_with("[scry] issue_types: `style` is not an issue type (bug, assumption, convention, design)\n"),
        "{}",
        r.err
    );
    assert!(run_logs(bad.path()).is_empty());
}

/// An exit code, stdout's last line and stderr's lines.
type Outcome = (i32, String, Vec<String>);

/// What `scry ARGS` gives in one row of the table.
fn outcome(dir: &Path, d: &Doubles, a: &[&str]) -> Outcome {
    let r = scry(dir, d, a);
    (
        r.code,
        r.out.lines().last().unwrap_or("").to_string(),
        r.err.lines().map(String::from).collect(),
    )
}

/// orders.py's change and a file of one 600-line unit, changed: two calls.
fn two_calls_project() -> TempDir {
    let dir = git_project();
    write(dir.path(), "orders.py", ORDERS);
    write(dir.path(), "x_big.py", &long_unit("xray", 600));
    commit(dir.path(), "two files");
    write(
        dir.path(),
        "orders.py",
        &ORDERS.replace("order.cancel()", "order.cancel(now=True)"),
    );
    let p = dir.path().join("x_big.py");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(p, text.replacen("x1 = 1", "x1 = 2", 1)).unwrap();
    dir
}

#[test]
fn exit_codes_and_held_back_lines_are_as_before() {
    let failing = || StubReviewer::new().reply(&["orders.py"], "I could not finish.");
    let mut rows: Vec<(&str, Outcome)> = Vec::new();

    let dir = changed_project();
    rows.push(("shown", outcome(dir.path(), &bug_doubles(dir.path()), &[])));
    let dir = changed_project();
    rows.push((
        "shown, --json",
        outcome(dir.path(), &bug_doubles(dir.path()), &["--json"]),
    ));
    let dir = changed_project();
    rows.push((
        "a failure",
        outcome(dir.path(), &Doubles::new(dir.path(), failing()), &[]),
    ));
    let dir = changed_project();
    rows.push((
        "a failure, --json",
        outcome(
            dir.path(),
            &Doubles::new(dir.path(), failing()),
            &["--json"],
        ),
    ));
    let dir = changed_project();
    rows.push((
        "capped by --budget",
        outcome(dir.path(), &bug_doubles(dir.path()), &["--budget", "0.01"]),
    ));
    let dir = changed_project();
    write(
        dir.path(),
        ".sorcerer/config.toml",
        "[scry]\nbudget_per_day = 0.01\n",
    );
    rows.push((
        "capped by the day",
        outcome(dir.path(), &bug_doubles(dir.path()), &[]),
    ));
    let dir = changed_project();
    rows.push((
        "a check capped by --budget",
        outcome(dir.path(), &bug_doubles(dir.path()), &["--budget", "0.05"]),
    ));
    let dir = two_calls_project();
    rows.push((
        "shown and capped",
        outcome(dir.path(), &bug_doubles(dir.path()), &["--budget", "0.07"]),
    ));
    let dir = changed_project();
    let quiet_reviewer = StubReviewer::new();
    rows.push((
        "nothing found",
        outcome(dir.path(), &Doubles::new(dir.path(), quiet_reviewer), &[]),
    ));
    let summary = "1 finding, 1 unit looked at, $0.04";
    let failed = "scry: review failed for orders.py: no JSON in the review reply";
    let json = r#"{"confidence":1.0,"evidence":[{"file":"orders.py","lines":[2,2]}],"file":"orders.py","id":"5034e6","issue_type":"bug","lines":[2,2],"rule":null,"statement":"cancel() passes now=True","status":"open","unit":"cancel","verdict":"right"}"#;
    let row = |code: i32, last: &str, err: &[&str]| {
        (
            code,
            last.to_string(),
            err.iter().map(|l| (*l).to_string()).collect::<Vec<_>>(),
        )
    };
    let want = [
        ("shown", row(1, summary, &[])),
        ("shown, --json", row(1, json, &[summary])),
        (
            "a failure",
            row(2, "0 findings, 0 units looked at, $0.04", &[failed]),
        ),
        (
            "a failure, --json",
            row(2, "", &["0 findings, 0 units looked at, $0.04", failed]),
        ),
        (
            "capped by --budget",
            row(
                2,
                "",
                &["scry: the budget or the day's cap stopped 1 unit; it waits for the next run"],
            ),
        ),
        (
            "capped by the day",
            row(
                2,
                "",
                &["scry: the day's cap stopped 1 unit; it waits for the next run"],
            ),
        ),
        (
            "a check capped by --budget",
            row(
                2,
                "0 findings, 1 unit looked at, $0.04",
                &[
                    "scry: the budget or the day's cap held back a claim check; its finding stays unverified until the next run",
                ],
            ),
        ),
        (
            "shown and capped",
            row(
                1,
                summary,
                &["scry: the budget or the day's cap stopped 1 unit; it waits for the next run"],
            ),
        ),
        (
            "nothing found",
            row(0, "0 findings, 1 unit looked at, $0.04", &[]),
        ),
    ];
    assert_eq!(rows.len(), want.len());
    for ((name, got), (want_name, want)) in rows.iter().zip(want.iter()) {
        assert_eq!(name, want_name);
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn doctor_output_is_as_recorded() {
    // no source file: no grammar is loaded (nor built into the working directory)
    let dir = git_project();
    let good = MapEnv::default()
        .on_path("claude")
        .var_set("JEV_API_KEY", "k");
    let r = doctor(dir.path(), &good);
    let want = "provider   ok    claude (claude is on PATH)\n\
                jev        ok    jev (JEV_API_KEY is set)\n\
                languages  ok    no file here is in a language sorcerer knows\n\
                steering   ok    no .sorcerer/scry.toml: no rules\n\
                store      ok    no findings yet\n\
                ledger     ok    0 rows, 0 of them scry's\n";
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, want, ""));
    let r = doctor(dir.path(), &MapEnv::default());
    let want = "provider   FAIL  no model provider: put claude on PATH or set ANTHROPIC_API_KEY\n\
                jev        FAIL  jev: no key; set JEV_API_KEY; scry does not run without Jev\n\
                languages  ok    no file here is in a language sorcerer knows\n\
                steering   ok    no .sorcerer/scry.toml: no rules\n\
                store      ok    no findings yet\n\
                ledger     ok    0 rows, 0 of them scry's\n";
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (2, want, ""));
}

#[test]
fn log_list_output_is_as_recorded() {
    let dir = changed_project();
    let d = bug_doubles(dir.path());
    scry(dir.path(), &d, &[]);
    scry(dir.path(), &d, &[]);
    let list = scry(dir.path(), &d, &["log", "--list"]);
    let want = "log                        time                        result\n\
                2026-11-18T23-00-00Z-2.md  2026-11-18 23:00:00 +00:00  1 finding shown, 0 units looked at, $0.00, exit 1\n\
                2026-11-18T23-00-00Z.md    2026-11-18 23:00:00 +00:00  1 finding shown, 1 unit looked at, $0.04, exit 1\n";
    assert_eq!(
        (list.code, list.out.as_str(), list.err.as_str()),
        (0, want, "")
    );
}

/// A store written before the word changed says "waiting": it reads as
/// unverified, and `--status waiting` still lists those findings.
#[test]
fn a_store_that_says_waiting_reads_as_unverified() {
    let dir = changed_project();
    let store = dir.path().join(".sorcerer/state/scry/findings.json");
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    let mut f = stored::finding(
        "eeee01",
        "bug",
        scry::domain::finding::Status::Unverified,
        "orders.py",
        2,
    );
    f.history.clear();
    let data = scry::domain::findings::Findings {
        findings: vec![f],
        ..Default::default()
    };
    let text = serde_json::json!({"version": 2, "data": data})
        .to_string()
        .replace("\"unverified\"", "\"waiting\"");
    assert!(text.contains("\"waiting\""), "{text}");
    std::fs::write(&store, text).unwrap();
    let d = plain_doubles(dir.path());
    for status in ["waiting", "unverified"] {
        let r = scry(dir.path(), &d, &["list", "--status", status]);
        assert_eq!(r.code, 0, "{}", r.err);
        assert!(r.out.contains("unverified  bug"), "{status}: {}", r.out);
    }
}
