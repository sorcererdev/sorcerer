mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use scry::config::ScryConfig;
use scry::finding::{Finding, Status};
use scry::pass::{Caps, Outcome, PassDeps, Progress, Report, pass};
use scry::scope::{ScopeWhy, scope_of};
use scry::store::{ScryStore, store};
use sorcerer_core::store::Snapshot;
use sorcerer_core::view::{CodebaseView, FilesView};
use sorcerer_core::{Cancel, Project};
use sorcerer_models::checker::{Checked, Claim};
use sorcerer_models::decide::Answer;
use support::{FixedClock, StubDecider, StubFinder, commit, git_project, write};
use tempfile::TempDir;

const ORDERS: &str = "def cancel(order):\n    return order.cancel()\n\n\ndef place(order):\n    return order.place()\n";

const STEERING: &str =
    "## Rules\n\n### Commands return a Result\nA command never panics; it returns an error.\n";

fn t0() -> DateTime<Utc> {
    "2026-11-18T23:00:00Z".parse().unwrap()
}

struct Fixture {
    dir: TempDir,
    project: Arc<Project>,
    view: FilesView,
}

impl Fixture {
    /// orders.py committed (with steering), then `cancel` changed.
    fn new() -> Fixture {
        let dir = git_project();
        write(dir.path(), ".sorcerer/scry.md", STEERING);
        write(dir.path(), "orders.py", ORDERS);
        commit(dir.path(), "orders");
        write(
            dir.path(),
            "orders.py",
            &ORDERS.replace("order.cancel()", "order.cancel(now=True)"),
        );
        let (project, view) = support::project_with_vendored_grammars(dir.path());
        Fixture { dir, project, view }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn run(&self, finder: &StubFinder, decider: &StubDecider, fresh: bool) -> Outcome {
        self.try_run(finder, decider, fresh).unwrap()
    }

    fn try_run(
        &self,
        finder: &StubFinder,
        decider: &StubDecider,
        fresh: bool,
    ) -> sorcerer_core::Result<Outcome> {
        let scope = scope_of(
            &self.project,
            &self.view,
            ScopeWhy::Working,
            false,
            &ScryConfig::default(),
        )
        .unwrap();
        let checker = |_: &Claim| -> sorcerer_models::Result<Checked> {
            panic!("task 7's pass runs no claim check")
        };
        let clock = FixedClock::at(t0());
        let progress = |_: &Progress| {};
        let deps = PassDeps {
            view: &self.view,
            finder,
            decider,
            checker: &checker,
            clock: &clock,
            progress: &progress,
        };
        let caps = Caps {
            pass: None,
            day: f64::INFINITY,
            budget: None,
        };
        pass(
            &self.project,
            &scope,
            &caps,
            fresh,
            &deps,
            &Cancel::default(),
        )
    }

    fn store(&self) -> ScryStore {
        match store(&self.project).unwrap().read().unwrap() {
            Snapshot::Current(s) => s,
            Snapshot::Absent => ScryStore::default(),
            Snapshot::Unreadable(why) => panic!("{why}"),
        }
    }

    fn store_path(&self) -> PathBuf {
        store(&self.project).unwrap().path().to_path_buf()
    }

    fn edit(&self, from: &str, to: &str) {
        let p = self.root().join("orders.py");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains(from), "{text}");
        std::fs::write(p, text.replace(from, to)).unwrap();
    }
}

fn ran(o: Outcome) -> Report {
    match o {
        Outcome::Ran(r) => r,
        Outcome::Skipped(why) => panic!("skipped: {why}"),
    }
}

/// A find reply holding one finding on orders.py with evidence at its lines.
fn reply(kind: &str, line: u32, statement: &str, rule: &str) -> String {
    serde_json::json!({
        "findings": [{
            "kind": kind,
            "file": "orders.py",
            "lines": [line, line],
            "statement": statement,
            "rule": rule,
            "evidence": [{"file": "orders.py", "lines": [line, line]}],
        }],
        "open": [],
    })
    .to_string()
}

fn only(s: &ScryStore) -> &Finding {
    assert_eq!(s.findings.len(), 1, "{:#?}", s.findings);
    &s.findings[0]
}

fn bug_finder(statement: &str) -> StubFinder {
    StubFinder::new().reply(&["orders.py"], &reply("bug", 2, statement, ""))
}

/// The fingerprint of `cancel` in orders.py as it is now.
fn cancel_print(f: &Fixture) -> String {
    let units = f.view.units(Path::new("orders.py")).unwrap();
    let u = units.iter().find(|u| u.name == "cancel").unwrap();
    u.fingerprint.to_string()
}

#[test]
fn a_clean_change_with_an_empty_reply_stores_nothing_and_marks_inspected() {
    let f = Fixture::new();
    let finder = StubFinder::new();
    let r = ran(f.run(&finder, &StubDecider::new(), false));
    assert_eq!(r.looked_at, 1);
    assert!(r.new.is_empty());
    assert!(r.failures.is_empty());
    assert!(r.whole);
    assert_eq!(r.capped, 0);
    assert!((r.spent - 0.04).abs() < 1e-9, "{}", r.spent);
    let ask = &finder.asks()[0];
    assert_eq!(ask.units.len(), 1);
    assert_eq!(ask.units[0].name, "cancel");
    assert!(ask.diff.contains("cancel(now=True)"), "{}", ask.diff);
    assert!(ask.steering.contains("### Commands return a Result"));
    assert!(ask.reported.is_empty() && ask.open.is_empty());
    let s = f.store();
    assert!(s.findings.is_empty());
    let inspected = &s.inspected[Path::new("orders.py")];
    assert!(inspected.contains(&cancel_print(&f)), "{inspected:?}");
}

#[test]
fn a_second_pass_sends_nothing_already_inspected() {
    let f = Fixture::new();
    let finder = StubFinder::new();
    ran(f.run(&finder, &StubDecider::new(), false));
    let r = ran(f.run(&finder, &StubDecider::new(), false));
    assert_eq!(finder.asks().len(), 1);
    assert_eq!(r.looked_at, 0);
    assert!(r.whole);
}

#[test]
fn fresh_sends_again() {
    let f = Fixture::new();
    let finder = StubFinder::new();
    ran(f.run(&finder, &StubDecider::new(), false));
    ran(f.run(&finder, &StubDecider::new(), true));
    assert_eq!(finder.asks().len(), 2);
}

#[test]
fn a_convention_with_citations_becomes_open() {
    let f = Fixture::new();
    let finder = StubFinder::new().reply(
        &["orders.py"],
        &reply(
            "convention",
            2,
            "cancel() can raise out of a command.",
            "commands return a result",
        ),
    );
    let r = ran(f.run(&finder, &StubDecider::new(), false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(r.new, std::slice::from_ref(&g.id));
    assert_eq!(g.status, Status::Open);
    assert_eq!(g.rule.as_deref(), Some("Commands return a Result"));
    assert_eq!(g.place.unit.as_deref(), Some("cancel"));
    assert_eq!(g.scores.keep, Some(1.0));
    assert!(g.scores.asked_on.is_some());
    assert!(g.evidence[0].print.is_some(), "{:?}", g.evidence);
    assert_eq!(g.seen, 1);
    assert_eq!(g.model, "sonnet");
}

#[test]
fn a_nitpick_is_withheld() {
    let f = Fixture::new();
    let decider = StubDecider::new().on("nitpick", |_| true, StubDecider::p_yes(0.9));
    let finder = bug_finder("cancel is a poor name.");
    ran(f.run(&finder, &decider, false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.status, Status::Withheld);
    assert!((g.scores.keep.unwrap() - 0.1).abs() < 1e-9);
    let asked = decider.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].kind, "nitpick");
}

#[test]
fn a_bug_waits_for_its_claim_check() {
    let f = Fixture::new();
    ran(f.run(
        &bug_finder("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.status, Status::Waiting);
    assert_eq!(g.history, [(t0(), Status::Waiting)]);
}

#[test]
fn a_finding_outside_the_scope_is_elsewhere() {
    let f = Fixture::new();
    let decider = StubDecider::new();
    ran(f.run(&bug_finder_at(6), &decider, false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.status, Status::Elsewhere);
    assert_eq!(g.place.unit.as_deref(), Some("place"));
    assert!(decider.asked().is_empty());
}

fn bug_finder_at(line: u32) -> StubFinder {
    StubFinder::new().reply(
        &["orders.py"],
        &reply("bug", line, "place() never checks stock.", ""),
    )
}

#[test]
fn an_unresolved_rule_is_dropped_and_the_finding_kept() {
    let f = Fixture::new();
    let finder = StubFinder::new().reply(
        &["orders.py"],
        &reply(
            "convention",
            2,
            "cancel() ignores the rule.",
            "Tabs, never spaces",
        ),
    );
    ran(f.run(&finder, &StubDecider::new(), false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.rule, None);
    assert_eq!(g.statement, "cancel() ignores the rule.");
}

#[test]
fn the_same_problem_merges_into_the_stored_id() {
    let f = Fixture::new();
    ran(f.run(
        &bug_finder("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let id = only(&f.store()).id.clone();
    let decider = StubDecider::new().on("same", |_| true, StubDecider::p_yes(0.8));
    let finder = bug_finder("cancel() gives back None, not the order.");
    let r = ran(f.run(&finder, &decider, true));
    assert!(r.new.is_empty());
    assert_eq!(finder.asks()[0].open.len(), 1);
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.id, id);
    assert_eq!(g.seen, 2);
    assert_eq!(g.statement, "cancel() gives back None, not the order.");
    assert_eq!(g.status, Status::Waiting);
    let kinds: Vec<String> = decider.asked().into_iter().map(|q| q.kind).collect();
    assert_eq!(kinds, ["same"]);
}

#[test]
fn dismissed_everywhere_drops_the_new_finding() {
    let f = Fixture::new();
    ran(f.run(
        &bug_finder("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let id = only(&f.store()).id.clone();
    store(&f.project)
        .unwrap()
        .update(|s| s.dismiss(&id.0, true, t0()))
        .unwrap()
        .unwrap();
    let decider = StubDecider::new().on("same", |_| true, StubDecider::p_yes(0.8));
    let r = ran(f.run(&bug_finder("cancel() returns None again."), &decider, true));
    assert!(r.new.is_empty());
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.status, Status::DismissedEverywhere);
    assert_eq!(g.statement, "cancel() returns None.");
}

#[test]
fn no_longer_holds_resolves() {
    let f = Fixture::new();
    ran(f.run(
        &bug_finder("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let id = only(&f.store()).id.clone();
    f.edit("cancel(now=True)", "cancel(now=True) or order");
    let answer = serde_json::json!({
        "findings": [],
        "open": [{"id": id.0, "holds": "no longer holds", "why": "it returns the order"}],
    })
    .to_string();
    let finder = StubFinder::new().reply(&["orders.py"], &answer);
    ran(f.run(&finder, &StubDecider::new(), false));
    assert_eq!(finder.asks()[0].open[0].id, id);
    assert_eq!(only(&f.store()).status, Status::Resolved);
}

#[test]
fn not_found_again_at_a_new_fingerprint_resolves_only_when_whole() {
    let f = Fixture::new();
    let big = format!("def big():\n{}", "    x = 1\n".repeat(600));
    write(f.root(), "big.py", &big);
    support::git(f.root(), &["add", "big.py"]);
    support::git(f.root(), &["commit", "-q", "-m", "big"]);
    f.edit("cancel(now=True)", "cancel(now=False)");
    ran(f.run(
        &bug_finder("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    assert_eq!(only(&f.store()).status, Status::Waiting);

    // cancel changes again and big too; big's call fails: nothing resolves
    f.edit("cancel(now=False)", "cancel(now=True)");
    write(f.root(), "big.py", &big.replace("x = 1", "x = 2"));
    let failing = StubFinder::new().reply(&["big.py"], "I could not finish.");
    let r = ran(f.run(&failing, &StubDecider::new(), false));
    assert_eq!(failing.asks().len(), 2);
    assert!(!r.whole);
    assert_eq!(r.failures.len(), 1);
    assert_eq!(only(&f.store()).status, Status::Waiting);

    // the same tree again, every call answering: not found again, resolved
    let r = ran(f.run(&StubFinder::new(), &StubDecider::new(), true));
    assert!(r.whole);
    assert_eq!(only(&f.store()).status, Status::Resolved);
}

#[test]
fn a_unit_removed_from_the_file_makes_its_findings_gone() {
    let f = Fixture::new();
    ran(f.run(
        &bug_finder("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    std::fs::write(
        f.root().join("orders.py"),
        "def place(order):\n    return order.place()\n",
    )
    .unwrap();
    ran(f.run(&StubFinder::new(), &StubDecider::new(), false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.status, Status::Gone);
    assert_eq!(g.history.last().unwrap().1, Status::Gone);
}

#[test]
fn a_find_reply_with_no_json_is_a_failure_and_leaves_units_uninspected() {
    let f = Fixture::new();
    let finder = StubFinder::new().reply(&["orders.py"], "Nothing to report, I think.");
    let r = ran(f.run(&finder, &StubDecider::new(), false));
    assert_eq!(r.failures.len(), 1);
    assert!(r.failures[0].contains("orders.py"), "{:?}", r.failures);
    assert!(r.failures[0].contains("no JSON"), "{:?}", r.failures);
    assert!(!r.whole);
    assert_eq!(r.looked_at, 0);
    assert!(f.store().inspected.is_empty());
    ran(f.run(&finder, &StubDecider::new(), false));
    assert_eq!(finder.asks().len(), 2);
}

#[test]
fn jev_silent_skips_and_writes_nothing() {
    let f = Fixture::new();
    ran(f.run(&StubFinder::new(), &StubDecider::new(), false));
    let before = std::fs::read(f.store_path()).unwrap();
    let decider = StubDecider::new().on("nitpick", |_| true, Answer::Unsure);
    match f.run(&bug_finder("cancel() returns None."), &decider, true) {
        Outcome::Skipped(why) => assert_eq!(why, "Jev did not answer"),
        Outcome::Ran(r) => panic!("ran: {r:?}"),
    }
    assert_eq!(std::fs::read(f.store_path()).unwrap(), before);
}

#[test]
fn a_dismissal_during_the_pass_stands() {
    let f = Fixture::new();
    ran(f.run(
        &bug_finder("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let id = only(&f.store()).id.clone();
    let project = f.project.clone();
    let dismissed = id.clone();
    let finder = bug_finder("cancel() gives back None.").on_find(move |_| {
        store(&project)
            .unwrap()
            .update(|s| s.dismiss(&dismissed.0, false, t0()))
            .unwrap()
            .unwrap();
    });
    let decider = StubDecider::new().on("same", |_| true, StubDecider::p_yes(0.8));
    ran(f.run(&finder, &decider, true));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.id, id);
    assert_eq!(g.status, Status::DismissedHere);
    assert!(!s.inspected.is_empty());
}

#[test]
fn an_unreadable_store_stops_the_pass_untouched() {
    let f = Fixture::new();
    let path = f.store_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = br#"{"version": 2, "data": {}}"#;
    std::fs::write(&path, bytes).unwrap();
    let finder = StubFinder::new();
    let e = f
        .try_run(&finder, &StubDecider::new(), false)
        .unwrap_err()
        .to_string();
    assert!(e.contains("version 2"), "{e}");
    assert!(finder.asks().is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
