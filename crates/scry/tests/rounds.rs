#![allow(clippy::too_many_lines)]
mod support;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Utc};
use scry::domain::budget::Caps;
use scry::domain::budget::{Budget, check_estimate, review_estimate};
use scry::domain::config::ScryConfig;
use scry::domain::finding::{Dismissal, Evidence, Finding, FindingId, Place, Scores, Status};
use scry::domain::findings::Findings;
use scry::domain::scope::ScopeRequest;
use scry::ports::claim_checker::ClaimChecker;
use scry::ports::run_log::Quiet;
use scry::usecases::review_change::{
    Progress, ReviewDeps, ReviewOutcome, ReviewReport, ReviewRequest, review_change,
};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Span;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::domain::claim::{Checked, Claim, Escalation, Verdict};
use support::{FixedClock, StubDecider, StubReviewer, commit, git_project, write};
use tempfile::TempDir;

const ORDERS: &str = "def cancel(order):\n    return order.cancel()\n\n\ndef place(order):\n    return order.place()\n";

const STEERING: &str = "[[rule]]\nid = \"result\"\ntext = \"\"\"\nCommands return a Result\nA command never panics; it returns an error.\"\"\"\n";

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

type CheckFn<'a> = dyn ClaimChecker + 'a;

struct Fixture {
    dir: TempDir,
    project: Arc<Project>,
    view: FilesView,
}

impl Fixture {
    /// orders.py committed (with steering), then `cancel` changed.
    fn new() -> Fixture {
        let dir = git_project();
        write(dir.path(), ".sorcerer/scry.toml", STEERING);
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

    fn run(&self, reviewer: &StubReviewer, checker: &CheckFn<'_>, caps: &Caps) -> ReviewReport {
        let progress = |_: &Progress| {};
        self.run_with(reviewer, checker, caps, &FixedClock::at(t0()), &progress)
    }

    fn run_with(
        &self,
        reviewer: &StubReviewer,
        checker: &CheckFn<'_>,
        caps: &Caps,
        clock: &FixedClock,
        progress: &(dyn Fn(&Progress) + Sync),
    ) -> ReviewReport {
        let decider = StubDecider::new();
        let ports = support::Ports::of(&self.project);
        let cancel = Cancel::default();
        let deps = ReviewDeps {
            view: &self.view,
            reviewer,
            decider: &decider,
            checker,
            store: &ports.store,
            ledger: &ports.ledger,
            settings: &ports.settings,
            clock,
            log: &Quiet,
            progress,
            cancel: &cancel,
        };
        let req = ReviewRequest {
            caps: Some(caps.clone()),
            ..ReviewRequest::of(ScopeRequest::Working)
        };
        match review_change(&self.project, &req, &deps).unwrap() {
            ReviewOutcome::Ran(r) => r,
            ReviewOutcome::Skipped(why) => panic!("skipped: {why}"),
        }
    }

    fn store(&self) -> Findings {
        read_store(&self.project)
    }

    fn ledger(&self) -> JsonlLedger {
        JsonlLedger::for_project(&self.project).unwrap()
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

    /// FINDINGS stored, with `cancel` already reviewed (nothing to review).
    fn seed(&self, findings: Vec<Finding>) {
        let print = self.cancel_print();
        support::update_store(&self.project, |s| {
            s.findings = findings;
            s.reviewed
                .entry("orders.py".into())
                .or_default()
                .insert(print);
        })
        .unwrap();
    }
}

fn read_store(p: &Project) -> Findings {
    support::read_store(p)
}

/// A finding on `cancel` citing line 2, with ID, ISSUE_TYPE, CONFIDENCE and STATUS.
fn finding(id: &str, issue_type: &str, confidence: f64, status: Status) -> Finding {
    let t = t0() - chrono::Duration::hours(1);
    Finding {
        id: FindingId(id.into()),
        issue_type: issue_type.into(),
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
            confidence: Some(confidence),
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
        .map(|(i, k)| finding(&format!("00000{i}"), "bug", *k, Status::Unverified))
        .collect()
}

fn status_of(s: &Findings, id: &str) -> Status {
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
        cited: Vec::new(),
        escalation: Escalation::No,
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
    assert_eq!(review_estimate(&[], "sonnet"), 0.04);
    assert_eq!(review_estimate(&[], "claude-haiku-5-5"), 0.005);
    assert_eq!(check_estimate(&[]), 0.016);

    let mut rows = Vec::new();
    for i in 1..=25 {
        rows.push(row("review", t0(), Some(i as f64 * 0.01)));
        rows.push(row("check", t0(), Some(i as f64 * 0.001)));
    }
    // unpriced rows, other verbs and other products do not count
    rows.push(row("review", t0(), None));
    rows.push(row("decide", t0(), Some(9.0)));
    let mut other = row("review", t0(), Some(9.0));
    other.product = "conjure".into();
    rows.push(other);
    // the last 20 of 1..=25 are 6..=25, mean 15.5
    assert!((review_estimate(&rows, "sonnet") - 0.155).abs() < 1e-9);
    assert!((review_estimate(&rows, "claude-haiku-5-5") - 0.155).abs() < 1e-9);
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
fn a_day_cap_stops_the_review_and_leaves_units_for_later() {
    let f = Fixture::new();
    f.ledger()
        .append(&row(
            "review",
            t0() - chrono::Duration::hours(1),
            Some(2.99),
        ))
        .unwrap();
    let reviewer = StubReviewer::new();
    let caps = Caps {
        pass: None,
        day: 3.00,
        budget: None,
    };
    let r = f.run(&reviewer, &right, &caps);
    assert!(reviewer.asks().is_empty());
    assert_eq!(r.capped, 1);
    assert!(!r.whole);
    assert_eq!(r.looked_at, 0);
    let s = f.store();
    assert!(s.reviewed.is_empty(), "{:?}", s.reviewed);
    assert!(s.capped[Path::new("orders.py")].contains("cancel"));

    // a later pass with room inspects it and clears the record
    let r = f.run(&reviewer, &right, &no_caps());
    assert_eq!(reviewer.asks().len(), 1);
    assert_eq!(r.capped, 0);
    let s = f.store();
    assert!(s.capped.is_empty(), "{:?}", s.capped);
    assert!(s.reviewed[Path::new("orders.py")].contains(&f.cancel_print()));
}

#[test]
fn a_budget_caps_the_run() {
    let f = Fixture::new();
    let big = format!("def big():\n{}", "    x = 1\n".repeat(600));
    write(f.dir.path(), "big.py", &big);
    support::git(f.dir.path(), &["add", "big.py"]);
    support::git(f.dir.path(), &["commit", "-q", "-m", "big"]);
    write(f.dir.path(), "big.py", &big.replace("x = 1", "x = 2"));
    let reviewer = StubReviewer::new();
    let caps = Caps {
        pass: None,
        day: f64::INFINITY,
        budget: Some(0.06),
    };
    let r = f.run(&reviewer, &right, &caps);
    assert_eq!(reviewer.asks().len(), 1);
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
            "review",
            "2026-11-18T03:00:00Z".parse().unwrap(),
            Some(2.99),
        ))
        .unwrap();
    // Jev's questions do not count against the caps
    f.ledger().append(&row("decide", t0(), Some(5.0))).unwrap();
    let reviewer = StubReviewer::new();
    let caps = Caps {
        pass: None,
        day: 3.00,
        budget: None,
    };
    let clock = FixedClock::at(t0()).in_zone(FixedOffset::west_opt(5 * 3600).unwrap());
    let progress = |_: &Progress| {};
    let r = f.run_with(&reviewer, &right, &caps, &clock, &progress);
    assert_eq!(reviewer.asks().len(), 1);
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
    let reviewer = StubReviewer::new();
    let r = f.run(&reviewer, &checker, &no_caps());
    assert!(reviewer.asks().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!((r.spent - 0.03).abs() < 1e-9, "{}", r.spent);
    let s = f.store();
    // keeps by id: 0 .85, 1 .95, 2 .75, 3 .90, 4 .80
    for (id, want) in [
        ("000001", Status::Open),
        ("000003", Status::Open),
        ("000000", Status::Open),
        ("000004", Status::Unverified),
        ("000002", Status::Unverified),
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
    f.run(&StubReviewer::new(), &checker, &no_caps());
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    let s = f.store();
    assert_eq!(status_of(&s, "000000"), Status::Refuted);
    for id in ["000001", "000002", "000003"] {
        assert_eq!(status_of(&s, id), Status::Open, "{id}");
    }
    assert_eq!(status_of(&s, "000004"), Status::Unverified);
}

#[test]
fn conventions_show_before_claim_checks_finish() {
    let f = Fixture::new();
    f.seed(vec![
        finding("00000b", "bug", 0.95, Status::Unverified),
        finding("00000c", "convention", 0.90, Status::Unverified),
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
        &StubReviewer::new(),
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
    f.run(&StubReviewer::new(), &checker, &no_caps());
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
    let r = f.run(&StubReviewer::new(), &failing, &no_caps());
    assert_eq!(r.spent, 0.0);
    let s = f.store();
    let g = &s.findings[0];
    assert_eq!(g.status, Status::Unverified);
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
    let r = f.run(&StubReviewer::new(), &checker, &caps);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(r.checks_capped);
    let s = f.store();
    assert_eq!(status_of(&s, "000000"), Status::Open);
    assert_eq!(status_of(&s, "000001"), Status::Unverified);
}

#[test]
fn a_finding_whose_citations_fail_is_unsure() {
    let f = Fixture::new();
    let mut g = finding("00000d", "bug", 0.95, Status::Unverified);
    g.evidence[0].lines = Span { start: 99, end: 99 };
    f.seed(vec![g]);
    let checker = |_: &Claim| -> sorcerer_models::Result<Checked> {
        panic!("no claim check when the citations fail")
    };
    f.run(&StubReviewer::new(), &checker, &no_caps());
    let s = f.store();
    assert_eq!(s.findings[0].status, Status::Unsure);
    assert_eq!(
        s.findings[0].verdict.as_ref().unwrap().verdict,
        "cited code not found"
    );
}

#[test]
fn a_waiting_finding_is_verified_when_room_frees_with_nothing_to_review() {
    let f = Fixture::new();
    let mut seeded: Vec<Finding> = ["00000e", "00000f", "000010"]
        .iter()
        .map(|id| finding(id, "bug", 0.99, Status::Open))
        .collect();
    seeded.push(finding("000011", "bug", 0.80, Status::Unverified));
    f.seed(seeded);
    let calls = AtomicUsize::new(0);
    let checker = |c: &Claim| {
        calls.fetch_add(1, Ordering::SeqCst);
        right(c)
    };
    let reviewer = StubReviewer::new();
    f.run(&reviewer, &checker, &no_caps());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(status_of(&f.store(), "000011"), Status::Unverified);

    support::update_store(&f.project, |s| s.dismiss("00000e", Dismissal::Here, t0()))
        .unwrap()
        .unwrap();
    f.run(&reviewer, &checker, &no_caps());
    assert!(reviewer.asks().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(status_of(&f.store(), "000011"), Status::Open);
}

/// A hung review call on the command line (no pass cap) is not presumed to
/// have spent the rest of the day: its cap, and its charge, stay within
/// `budget_per_pass`.
#[test]
fn a_timed_out_review_is_charged_at_most_the_budget_per_pass() {
    let f = Fixture::new();
    let reviewer = StubReviewer::new().timing_out();
    let caps = Caps {
        pass: None,
        day: 3.00,
        budget: None,
    };
    let no_check = |_: &Claim| -> sorcerer_models::Result<Checked> { unreachable!() };
    let r = f.run(&reviewer, &no_check, &caps);
    let budget_per_pass = ScryConfig::default().budget_per_pass;
    assert_eq!(r.failures.len(), 1, "{:?}", r.failures);
    assert!(reviewer.caps.lock().unwrap()[0] <= budget_per_pass);
    assert!(r.spent <= budget_per_pass, "charged {}", r.spent);
    assert!(r.spent > 0.0);
}

/// A pass over F with REVIEWER and CHECKER under CAPS: its report, its run
/// log and its progress events.
fn run_logged(
    f: &Fixture,
    reviewer: &StubReviewer,
    checker: &CheckFn<'_>,
    caps: &Caps,
) -> (ReviewReport, String, Vec<Progress>) {
    let log = Mutex::new(String::new());
    let trace = |s: &str| log.lock().unwrap().push_str(s);
    let progress = Mutex::new(Vec::new());
    let on_progress = |p: &Progress| progress.lock().unwrap().push(p.clone());
    let decider = StubDecider::new();
    let ports = support::Ports::of(&f.project);
    let cancel = Cancel::default();
    let clock = FixedClock::at(t0());
    let deps = ReviewDeps {
        view: &f.view,
        reviewer,
        decider: &decider,
        checker,
        store: &ports.store,
        ledger: &ports.ledger,
        settings: &ports.settings,
        clock: &clock,
        log: &trace,
        progress: &on_progress,
        cancel: &cancel,
    };
    let req = ReviewRequest {
        caps: Some(caps.clone()),
        ..ReviewRequest::of(ScopeRequest::Working)
    };
    let report = match review_change(&f.project, &req, &deps).unwrap() {
        ReviewOutcome::Ran(r) => r,
        ReviewOutcome::Skipped(why) => panic!("skipped: {why}"),
    };
    (
        report,
        log.into_inner().unwrap(),
        progress.into_inner().unwrap(),
    )
}

/// The run log's sections from the first `## Verification:` heading to
/// the last one's end.
fn verification_text(log: &str) -> &str {
    let start = log
        .find("\n## Verification: ")
        .unwrap_or_else(|| panic!("no Verification section in:\n{log}"));
    let last = log.rfind("\n## Verification: ").unwrap();
    let end = log[last + 1..]
        .find("\n## ")
        .map_or(log.len(), |e| last + 1 + e);
    &log[start..end]
}

#[test]
fn the_run_log_of_the_rounds_is_as_recorded() {
    use sorcerer_models::domain::claim::{CitationCheck, Cited, RawCitation};
    let f = Fixture::new();
    // six findings waiting before the pass, `cancel` not yet reviewed
    let mut unsure = finding("00000a", "bug", 0.95, Status::Unverified);
    unsure.evidence.push(Evidence {
        file: "orders.py".into(),
        lines: Span { start: 99, end: 99 },
        print: None,
    });
    let seeded = vec![
        unsure,
        finding("00000b", "convention", 0.92, Status::Unverified),
        finding("00000c", "bug", 0.90, Status::Unverified),
        finding("00000d", "bug", 0.80, Status::Unverified),
        finding("00000e", "bug", 0.70, Status::Unverified),
        // left for a round after the hold: no later round may take it
        finding("00000f", "bug", 0.60, Status::Unverified),
    ];
    support::update_store(&f.project, |s| s.findings = seeded).unwrap();
    // and one the pass finds
    let reply = serde_json::json!({
        "findings": [{
            "issue_type": "bug",
            "file": "orders.py",
            "lines": [2, 2],
            "statement": "cancel forgets the order",
            "rule": "",
            "evidence": [{"file": "orders.py", "lines": [2, 2]}],
        }],
        "open": [],
    })
    .to_string();
    let reviewer = StubReviewer::new().reply(&["orders.py"], &reply);
    let checker = |c: &Claim| -> sorcerer_models::Result<Checked> {
        match c.statement.as_str() {
            "cancel forgets the order" => Ok(Checked {
                cited: vec![Cited {
                    raw: RawCitation {
                        path: "orders.py".into(),
                        unit: Some("cancel".into()),
                        line: Some(2),
                    },
                    check: CitationCheck::Confirmed,
                }],
                ..checked(Verdict::Right)
            }),
            "claim 00000d" => Ok(checked(Verdict::Wrong)),
            _ => failing(c),
        }
    };
    // the review ($0.04) and three checks fit; a fourth's estimate does not
    let caps = Caps {
        pass: Some(0.07),
        day: f64::INFINITY,
        budget: None,
    };
    let (r, log, progress) = run_logged(&f, &reviewer, &checker, &caps);
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/run-log-verify.md");
    let text = verification_text(&log);
    if std::env::var_os("SCRY_BLESS").is_some() {
        std::fs::write(&golden_path, text).unwrap();
    }
    let golden = std::fs::read_to_string(&golden_path).unwrap();
    assert_eq!(
        text, golden,
        "the run log drifted; SCRY_BLESS=1 rewrites the golden file"
    );

    assert!(r.checks_capped);
    assert_eq!(r.new.len(), 1);
    assert_eq!(
        progress,
        [
            Progress::Reviewed(1),
            Progress::Stored,
            Progress::Stored,
            Progress::Stored,
            Progress::Stored,
        ]
    );
    let s = f.store();
    assert_eq!(status_of(&s, &r.new[0].0), Status::Open);
    assert_eq!(status_of(&s, "00000a"), Status::Unsure);
    assert_eq!(status_of(&s, "00000b"), Status::Open);
    assert_eq!(status_of(&s, "00000c"), Status::Unverified);
    assert_eq!(status_of(&s, "00000d"), Status::Refuted);
    assert_eq!(status_of(&s, "00000e"), Status::Unverified);
    assert_eq!(status_of(&s, "00000f"), Status::Unverified);
    assert!(!log.contains("### Round 5"), "{log}");
}
