mod support;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Utc};
use scry::config::ScryConfig;
use scry::finding::{Evidence, Finding, Id, Place, Scores, Status};
use scry::pass::{Caps, Outcome, PassDeps, Progress, Report, pass};
use scry::rounds::{Budget, check_estimate, find_estimate};
use scry::scope::{ScopeWhy, scope_of};
use scry::store::{ScryStore, store};
use sorcerer_core::ledger::{Errand, Ledger};
use sorcerer_core::store::Snapshot;
use sorcerer_core::unit::Span;
use sorcerer_core::view::{CodebaseView, FilesView};
use sorcerer_core::{Cancel, Project};
use sorcerer_models::checker::{Checked, Claim, Verdict};
use support::{FixedClock, StubDecider, StubFinder, commit, git_project, write};
use tempfile::TempDir;

const ORDERS: &str = "def cancel(order):\n    return order.cancel()\n\n\ndef place(order):\n    return order.place()\n";

const STEERING: &str =
    "## Rules\n\n### Commands return a Result\nA command never panics; it returns an error.\n";

fn t0() -> DateTime<Utc> {
    "2026-11-18T23:00:00Z".parse().unwrap()
}

fn no_caps() -> Caps {
    Caps {
        pass: None,
        day: f64::INFINITY,
        budget: None,
    }
}

type CheckFn<'a> = dyn Fn(&Claim) -> sorcerer_models::Result<Checked> + Sync + 'a;

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

    fn run(&self, finder: &StubFinder, checker: &CheckFn<'_>, caps: &Caps) -> Report {
        let progress = |_: &Progress| {};
        self.run_with(finder, checker, caps, &FixedClock::at(t0()), &progress)
    }

    fn run_with(
        &self,
        finder: &StubFinder,
        checker: &CheckFn<'_>,
        caps: &Caps,
        clock: &FixedClock,
        progress: &(dyn Fn(&Progress) + Sync),
    ) -> Report {
        let scope = scope_of(
            &self.project,
            &self.view,
            ScopeWhy::Working,
            false,
            &ScryConfig::default(),
        )
        .unwrap();
        let decider = StubDecider::new();
        let deps = PassDeps {
            view: &self.view,
            finder,
            decider: &decider,
            checker,
            clock,
            progress,
        };
        match pass(
            &self.project,
            &scope,
            caps,
            false,
            &deps,
            &Cancel::default(),
        )
        .unwrap()
        {
            Outcome::Ran(r) => r,
            Outcome::Skipped(why) => panic!("skipped: {why}"),
        }
    }

    fn store(&self) -> ScryStore {
        read_store(&self.project)
    }

    fn ledger(&self) -> Ledger {
        Ledger::for_project(&self.project).unwrap()
    }

    /// The fingerprint of `cancel` as it is now.
    fn cancel_print(&self) -> String {
        let units = self.view.units(Path::new("orders.py")).unwrap();
        units
            .iter()
            .find(|u| u.name == "cancel")
            .unwrap()
            .fingerprint
            .to_string()
    }

    /// FINDINGS stored, with `cancel` already inspected (nothing to find).
    fn seed(&self, findings: Vec<Finding>) {
        let print = self.cancel_print();
        store(&self.project)
            .unwrap()
            .update(|s| {
                s.findings = findings;
                s.inspected
                    .entry("orders.py".into())
                    .or_default()
                    .insert(print);
            })
            .unwrap();
    }
}

fn read_store(p: &Project) -> ScryStore {
    match store(p).unwrap().read().unwrap() {
        Snapshot::Current(s) => s,
        Snapshot::Absent => ScryStore::default(),
        Snapshot::Unreadable(why) => panic!("{why}"),
    }
}

/// A finding on `cancel` citing line 2, with ID, KIND, KEEP and STATUS.
fn finding(id: &str, kind: &str, keep: f64, status: Status) -> Finding {
    let t = t0() - chrono::Duration::hours(1);
    Finding {
        id: Id(id.into()),
        kind: kind.into(),
        rule: None,
        statement: format!("claim {id}"),
        place: Place {
            file: "orders.py".into(),
            unit: Some("cancel".into()),
            lines: Span { start: 2, end: 2 },
            unit_start: Some(1),
            fingerprint: None,
        },
        evidence: vec![Evidence {
            file: "orders.py".into(),
            lines: Span { start: 2, end: 2 },
            print: None,
        }],
        scores: Scores {
            keep: Some(keep),
            asked_on: Some("h".into()),
        },
        verdict: None,
        status,
        seen: 1,
        first: t,
        last: t,
        history: vec![(t, status)],
        model: "sonnet".into(),
        cost_usd: 0.04,
    }
}

fn waiting_bugs(keeps: &[f64]) -> Vec<Finding> {
    keeps
        .iter()
        .enumerate()
        .map(|(i, k)| finding(&format!("00000{i}"), "bug", *k, Status::Waiting))
        .collect()
}

fn status_of(s: &ScryStore, id: &str) -> Status {
    s.findings.iter().find(|f| f.id.0 == id).unwrap().status
}

fn row(verb: &str, at: DateTime<Utc>, cost: Option<f64>) -> Errand {
    let mut e = support::errand(verb, "sonnet", 0.0);
    e.at = at;
    e.cost_usd = cost;
    e
}

fn checked(verdict: Verdict) -> Checked {
    Checked {
        verdict,
        why: format!("{verdict:?}"),
        citations: Vec::new(),
        escalated: false,
        errands: vec![support::errand("check", "haiku", 0.01)],
    }
}

fn right(_: &Claim) -> sorcerer_models::Result<Checked> {
    Ok(checked(Verdict::Right))
}

fn failing(_: &Claim) -> sorcerer_models::Result<Checked> {
    Err(sorcerer_models::Error::Reply {
        provider: "stub".into(),
        why: "no answer".into(),
    })
}

#[test]
fn estimates_default_then_follow_the_record() {
    assert_eq!(find_estimate(&[], "sonnet"), 0.04);
    assert_eq!(find_estimate(&[], "claude-haiku-5-5"), 0.005);
    assert_eq!(check_estimate(&[]), 0.016);

    let mut rows = Vec::new();
    for i in 1..=25 {
        rows.push(row("find", t0(), Some(i as f64 * 0.01)));
        rows.push(row("check", t0(), Some(i as f64 * 0.001)));
    }
    // unpriced rows, other verbs and other products do not count
    rows.push(row("find", t0(), None));
    rows.push(row("decide", t0(), Some(9.0)));
    let mut other = row("find", t0(), Some(9.0));
    other.product = "conjure".into();
    rows.push(other);
    // the last 20 of 1..=25 are 6..=25, mean 15.5
    assert!((find_estimate(&rows, "sonnet") - 0.155).abs() < 1e-9);
    assert!((find_estimate(&rows, "claude-haiku-5-5") - 0.155).abs() < 1e-9);
    assert!((check_estimate(&rows) - 0.0155).abs() < 1e-9);
}

#[test]
fn a_budget_is_over_on_any_cap() {
    let caps = Caps {
        pass: Some(0.25),
        day: 3.0,
        budget: Some(0.10),
    };
    let mut b = Budget::new(&caps, 2.90);
    assert!(b.allows(0.05));
    b.charge(0.05);
    assert!(b.allows(0.05));
    assert!(!b.allows(0.06));
    let mut day = Budget::new(&no_caps(), 0.0);
    day.charge(100.0);
    assert!(day.allows(1.0));
    assert!(!Budget::new(&caps, 2.99).allows(0.04));
}

#[test]
fn a_day_cap_stops_the_find_and_leaves_units_for_later() {
    let f = Fixture::new();
    f.ledger()
        .append(&row("find", t0() - chrono::Duration::hours(1), Some(2.99)))
        .unwrap();
    let finder = StubFinder::new();
    let caps = Caps {
        pass: None,
        day: 3.00,
        budget: None,
    };
    let r = f.run(&finder, &right, &caps);
    assert!(finder.asks().is_empty());
    assert_eq!(r.capped, 1);
    assert!(!r.whole);
    assert_eq!(r.looked_at, 0);
    let s = f.store();
    assert!(s.inspected.is_empty(), "{:?}", s.inspected);
    assert!(s.capped[Path::new("orders.py")].contains("cancel"));

    // a later pass with room inspects it and clears the record
    let r = f.run(&finder, &right, &no_caps());
    assert_eq!(finder.asks().len(), 1);
    assert_eq!(r.capped, 0);
    let s = f.store();
    assert!(s.capped.is_empty(), "{:?}", s.capped);
    assert!(s.inspected[Path::new("orders.py")].contains(&f.cancel_print()));
}

#[test]
fn a_budget_caps_the_run() {
    let f = Fixture::new();
    let big = format!("def big():\n{}", "    x = 1\n".repeat(600));
    write(f.dir.path(), "big.py", &big);
    support::git(f.dir.path(), &["add", "big.py"]);
    support::git(f.dir.path(), &["commit", "-q", "-m", "big"]);
    write(f.dir.path(), "big.py", &big.replace("x = 1", "x = 2"));
    let finder = StubFinder::new();
    let caps = Caps {
        pass: None,
        day: f64::INFINITY,
        budget: Some(0.06),
    };
    let r = f.run(&finder, &right, &caps);
    assert_eq!(finder.asks().len(), 1);
    assert_eq!(r.looked_at, 1);
    assert_eq!(r.capped, 1);
    assert!((r.spent - 0.04).abs() < 1e-9, "{}", r.spent);
    assert!(!r.whole);
    assert_eq!(f.store().capped.len(), 1);
}

#[test]
fn yesterdays_spend_does_not_count() {
    let f = Fixture::new();
    // 03:00 UTC on the 18th is 22:00 on the 17th in New York
    f.ledger()
        .append(&row(
            "find",
            "2026-11-18T03:00:00Z".parse().unwrap(),
            Some(2.99),
        ))
        .unwrap();
    // Jev's questions do not count against the caps
    f.ledger().append(&row("decide", t0(), Some(5.0))).unwrap();
    let finder = StubFinder::new();
    let caps = Caps {
        pass: None,
        day: 3.00,
        budget: None,
    };
    let clock = FixedClock::at(t0()).in_zone(FixedOffset::west_opt(5 * 3600).unwrap());
    let progress = |_: &Progress| {};
    let r = f.run_with(&finder, &right, &caps, &clock, &progress);
    assert_eq!(finder.asks().len(), 1);
    assert_eq!(r.capped, 0);
}

#[test]
fn rounds_fill_max_shown_best_first() {
    let f = Fixture::new();
    f.seed(waiting_bugs(&[0.85, 0.95, 0.75, 0.90, 0.80]));
    let calls = AtomicUsize::new(0);
    let checker = |c: &Claim| {
        calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(c.about.len(), 1);
        assert!(c.about[0].print.is_some());
        right(c)
    };
    let finder = StubFinder::new();
    let r = f.run(&finder, &checker, &no_caps());
    assert!(finder.asks().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!((r.spent - 0.03).abs() < 1e-9, "{}", r.spent);
    let s = f.store();
    // keeps by id: 0 .85, 1 .95, 2 .75, 3 .90, 4 .80
    for (id, want) in [
        ("000001", Status::Open),
        ("000003", Status::Open),
        ("000000", Status::Open),
        ("000004", Status::Waiting),
        ("000002", Status::Waiting),
    ] {
        assert_eq!(status_of(&s, id), want, "{id}");
    }
    let g = s.findings.iter().find(|g| g.id.0 == "000001").unwrap();
    assert_eq!(g.verdict.as_ref().unwrap().verdict, "right");
    assert!((g.cost_usd - 0.05).abs() < 1e-9);
    assert_eq!(g.history.last(), Some(&(t0(), Status::Open)));
}

#[test]
fn a_refuted_check_frees_its_place() {
    let f = Fixture::new();
    f.seed(waiting_bugs(&[0.95, 0.90, 0.85, 0.80, 0.75]));
    let calls = AtomicUsize::new(0);
    let checker = |c: &Claim| {
        calls.fetch_add(1, Ordering::SeqCst);
        if c.statement == "claim 000000" {
            Ok(checked(Verdict::Wrong))
        } else {
            right(c)
        }
    };
    f.run(&StubFinder::new(), &checker, &no_caps());
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    let s = f.store();
    assert_eq!(status_of(&s, "000000"), Status::Refuted);
    for id in ["000001", "000002", "000003"] {
        assert_eq!(status_of(&s, id), Status::Open, "{id}");
    }
    assert_eq!(status_of(&s, "000004"), Status::Waiting);
}

#[test]
fn conventions_show_before_claim_checks_finish() {
    let f = Fixture::new();
    f.seed(vec![
        finding("00000b", "bug", 0.95, Status::Waiting),
        finding("00000c", "convention", 0.90, Status::Waiting),
    ]);
    let shown_early = Arc::new(AtomicBool::new(false));
    let project = f.project.clone();
    let seen = shown_early.clone();
    let progress = move |p: &Progress| {
        if *p == Progress::Stored && status_of(&read_store(&project), "00000c") == Status::Open {
            seen.store(true, Ordering::SeqCst);
        }
    };
    let checked_after = AtomicBool::new(false);
    let checker = |c: &Claim| {
        checked_after.store(shown_early.load(Ordering::SeqCst), Ordering::SeqCst);
        right(c)
    };
    f.run_with(
        &StubFinder::new(),
        &checker,
        &no_caps(),
        &FixedClock::at(t0()),
        &progress,
    );
    assert!(checked_after.load(Ordering::SeqCst));
    let s = f.store();
    assert_eq!(status_of(&s, "00000b"), Status::Open);
    assert_eq!(status_of(&s, "00000c"), Status::Open);
}

#[test]
fn checks_run_side_by_side() {
    let f = Fixture::new();
    f.seed(waiting_bugs(&[0.95, 0.90, 0.85]));
    let spans: Mutex<Vec<(Instant, Instant)>> = Mutex::new(Vec::new());
    let checker = |c: &Claim| {
        let start = Instant::now();
        std::thread::sleep(Duration::from_millis(200));
        spans.lock().unwrap().push((start, Instant::now()));
        right(c)
    };
    f.run(&StubFinder::new(), &checker, &no_caps());
    let spans = spans.into_inner().unwrap();
    assert_eq!(spans.len(), 3);
    let first = spans.iter().map(|s| s.0).min().unwrap();
    let last = spans.iter().map(|s| s.1).max().unwrap();
    assert!(
        last - first < Duration::from_millis(500),
        "{:?}",
        last - first
    );
}

#[test]
fn a_check_error_leaves_the_finding_waiting() {
    let f = Fixture::new();
    f.seed(waiting_bugs(&[0.95]));
    let r = f.run(&StubFinder::new(), &failing, &no_caps());
    assert_eq!(r.spent, 0.0);
    let s = f.store();
    let g = &s.findings[0];
    assert_eq!(g.status, Status::Waiting);
    assert_eq!(g.verdict, None);
    assert_eq!(g.history.len(), 1);
}

#[test]
fn a_check_that_does_not_fit_waits_and_stops_the_rounds() {
    let f = Fixture::new();
    f.seed(waiting_bugs(&[0.95, 0.90]));
    let caps = Caps {
        pass: Some(0.02),
        day: f64::INFINITY,
        budget: None,
    };
    let calls = AtomicUsize::new(0);
    let checker = |c: &Claim| {
        calls.fetch_add(1, Ordering::SeqCst);
        right(c)
    };
    let r = f.run(&StubFinder::new(), &checker, &caps);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(r.checks_capped);
    let s = f.store();
    assert_eq!(status_of(&s, "000000"), Status::Open);
    assert_eq!(status_of(&s, "000001"), Status::Waiting);
}

#[test]
fn a_finding_whose_citations_fail_is_unsure() {
    let f = Fixture::new();
    let mut g = finding("00000d", "bug", 0.95, Status::Waiting);
    g.evidence[0].lines = Span { start: 99, end: 99 };
    f.seed(vec![g]);
    let checker = |_: &Claim| -> sorcerer_models::Result<Checked> {
        panic!("no claim check when the citations fail")
    };
    f.run(&StubFinder::new(), &checker, &no_caps());
    let s = f.store();
    assert_eq!(s.findings[0].status, Status::Unsure);
    assert_eq!(
        s.findings[0].verdict.as_ref().unwrap().verdict,
        "cited code not found"
    );
}

#[test]
fn a_waiting_finding_is_verified_when_room_frees_with_nothing_to_find() {
    let f = Fixture::new();
    let mut seeded: Vec<Finding> = ["00000e", "00000f", "000010"]
        .iter()
        .map(|id| finding(id, "bug", 0.99, Status::Open))
        .collect();
    seeded.push(finding("000011", "bug", 0.80, Status::Waiting));
    f.seed(seeded);
    let calls = AtomicUsize::new(0);
    let checker = |c: &Claim| {
        calls.fetch_add(1, Ordering::SeqCst);
        right(c)
    };
    let finder = StubFinder::new();
    f.run(&finder, &checker, &no_caps());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(status_of(&f.store(), "000011"), Status::Waiting);

    store(&f.project)
        .unwrap()
        .update(|s| s.dismiss("00000e", false, t0()))
        .unwrap()
        .unwrap();
    f.run(&finder, &checker, &no_caps());
    assert!(finder.asks().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(status_of(&f.store(), "000011"), Status::Open);
}
