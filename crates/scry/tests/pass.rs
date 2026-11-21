#![allow(clippy::too_many_lines)]
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use scry::domain::budget::Caps;
use scry::domain::finding::{Dismissal, Finding, Status};
use scry::domain::findings::Findings;
use scry::domain::findings::{merge_into, renamed};
use scry::domain::scope::ScopeRequest;
use scry::ports::finding_store::FindingStore;
use scry::ports::run_log::Quiet;
use scry::usecases::review_change::{
    Progress, ReviewDeps, ReviewOutcome, ReviewReport, ReviewRequest, review_change,
};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::json_store::Snapshot;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::domain::claim::{Checked, Claim, Escalation};
use sorcerer_models::domain::decide::Answer;
use support::{FixedClock, StubDecider, StubReviewer, commit, git_project, write};
use tempfile::TempDir;

const ORDERS: &str = "def cancel(order):\n    return order.cancel()\n\n\ndef place(order):\n    return order.place()\n";

const STEERING: &str = "[[rule]]\nid = \"result\"\ntext = \"\"\"\nCommands return a Result\nA command never panics; it returns an error.\"\"\"\n\n[[rule]]\nid = \"clock-port\"\ntext = \"Time comes from the clock port.\"\n";

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

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn run(&self, reviewer: &StubReviewer, decider: &StubDecider, fresh: bool) -> ReviewOutcome {
        self.try_run(reviewer, decider, fresh).unwrap()
    }

    fn try_run(
        &self,
        reviewer: &StubReviewer,
        decider: &StubDecider,
        fresh: bool,
    ) -> sorcerer_core::Result<ReviewOutcome> {
        // a claim check that fails: an unverified finding stays unverified
        let checker = |_: &Claim| -> sorcerer_models::Result<Checked> {
            Err(sorcerer_models::Error::Reply {
                provider: "stub".into(),
                why: "no claim check in these tests".into(),
            })
        };
        let clock = FixedClock::at(t0());
        let progress = |_: &Progress| {};
        let ports = support::Ports::of(&self.project);
        let cancel = Cancel::default();
        let deps = ReviewDeps {
            view: &self.view,
            reviewer,
            decider,
            checker: &checker,
            store: &ports.store,
            ledger: &ports.ledger,
            settings: &ports.settings,
            clock: &clock,
            log: &Quiet,
            progress: &progress,
            cancel: &cancel,
            review_cancel: &cancel,
        };
        let req = ReviewRequest {
            fresh,
            caps: Some(Caps {
                pass: None,
                day: f64::INFINITY,
                budget: None,
            }),
            ..ReviewRequest::of(ScopeRequest::Working)
        };
        review_change(&self.project, &req, &deps)
    }

    fn store(&self) -> Findings {
        match support::store_of(&self.project).read().unwrap() {
            Snapshot::Current(s) => s,
            Snapshot::Absent => Findings::default(),
            Snapshot::Unreadable(why) => panic!("{why}"),
        }
    }

    fn store_path(&self) -> PathBuf {
        support::store_of(&self.project).path().to_path_buf()
    }

    fn edit(&self, from: &str, to: &str) {
        let p = self.root().join("orders.py");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains(from), "{text}");
        std::fs::write(p, text.replace(from, to)).unwrap();
    }
}

fn ran(o: ReviewOutcome) -> ReviewReport {
    match o {
        ReviewOutcome::Ran(r) => r,
        ReviewOutcome::Skipped(why) => panic!("skipped: {why}"),
    }
}

/// A review reply holding one finding on orders.py with evidence at its lines.
fn reply(kind: &str, line: u32, statement: &str, rule: &str) -> String {
    serde_json::json!({
        "findings": [{
            "issue_type": kind,
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

fn only(s: &Findings) -> &Finding {
    assert_eq!(s.findings.len(), 1, "{:#?}", s.findings);
    &s.findings[0]
}

fn bug_reviewer(statement: &str) -> StubReviewer {
    StubReviewer::new().reply(&["orders.py"], &reply("bug", 2, statement, ""))
}

/// The fingerprint of `cancel` in orders.py as it is now.
fn cancel_print(f: &Fixture) -> String {
    let units = f.view.units(Path::new("orders.py")).unwrap();
    let u = units.iter().find(|u| u.name == "cancel").unwrap();
    u.fingerprint.to_string()
}

#[test]
fn a_clean_change_with_an_empty_reply_stores_nothing_and_marks_reviewed() {
    let f = Fixture::new();
    let reviewer = StubReviewer::new();
    let r = ran(f.run(&reviewer, &StubDecider::new(), false));
    assert_eq!(r.looked_at, 1);
    assert!(r.new.is_empty());
    assert!(r.failures.is_empty());
    assert!(r.whole);
    assert_eq!(r.capped, 0);
    assert!((r.spent - 0.04).abs() < 1e-9, "{}", r.spent);
    let ask = &reviewer.asks()[0];
    assert_eq!(ask.units.len(), 1);
    assert_eq!(ask.units[0].name, "cancel");
    assert!(ask.diff.contains("cancel(now=True)"), "{}", ask.diff);
    assert!(ask.steering.contains("- result: Commands return a Result"));
    assert!(ask.reported.is_empty() && ask.open.is_empty());
    let s = f.store();
    assert!(s.findings.is_empty());
    let reviewed = &s.reviewed[Path::new("orders.py")];
    assert!(reviewed.contains(&cancel_print(&f)), "{reviewed:?}");
}

#[test]
fn a_second_pass_sends_nothing_already_reviewed() {
    let f = Fixture::new();
    let reviewer = StubReviewer::new();
    ran(f.run(&reviewer, &StubDecider::new(), false));
    let r = ran(f.run(&reviewer, &StubDecider::new(), false));
    assert_eq!(reviewer.asks().len(), 1);
    assert_eq!(r.looked_at, 0);
    assert!(r.whole);
}

#[test]
fn fresh_sends_again() {
    let f = Fixture::new();
    let reviewer = StubReviewer::new();
    ran(f.run(&reviewer, &StubDecider::new(), false));
    ran(f.run(&reviewer, &StubDecider::new(), true));
    assert_eq!(reviewer.asks().len(), 2);
}

#[test]
fn a_convention_with_citations_becomes_open() {
    let f = Fixture::new();
    let reviewer = StubReviewer::new().reply(
        &["orders.py"],
        &reply(
            "convention",
            2,
            "cancel() can raise out of a command.",
            "result",
        ),
    );
    let r = ran(f.run(&reviewer, &StubDecider::new(), false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(r.new, std::slice::from_ref(&g.id));
    assert_eq!(g.status, Status::Open);
    assert_eq!(g.rule.as_deref(), Some("result"));
    assert_eq!(g.place.unit.as_deref(), Some("cancel"));
    assert_eq!(g.scores.confidence, Some(1.0));
    assert!(g.scores.asked_on.is_some());
    assert!(g.evidence[0].print.is_some(), "{:?}", g.evidence);
    assert_eq!(g.seen, 1);
    assert_eq!(g.model, "sonnet");
}

#[test]
fn a_nitpick_is_withheld() {
    let f = Fixture::new();
    let decider = StubDecider::new().on("nitpick", |_| true, StubDecider::p_yes(0.9));
    let reviewer = bug_reviewer("cancel is a poor name.");
    ran(f.run(&reviewer, &decider, false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.status, Status::Withheld);
    assert!((g.scores.confidence.unwrap() - 0.1).abs() < 1e-9);
    let asked = decider.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].kind, "nitpick");
}

#[test]
fn a_bug_waits_for_its_claim_check() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.status, Status::Unverified);
    assert_eq!(g.history, [(t0(), Status::Unverified)]);
}

#[test]
fn a_finding_outside_the_scope_is_elsewhere() {
    let f = Fixture::new();
    let decider = StubDecider::new();
    ran(f.run(&bug_reviewer_at(6), &decider, false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.status, Status::Elsewhere);
    assert_eq!(g.place.unit.as_deref(), Some("place"));
    assert!(decider.asked().is_empty());
}

fn bug_reviewer_at(line: u32) -> StubReviewer {
    StubReviewer::new().reply(
        &["orders.py"],
        &reply("bug", line, "place() never checks stock.", ""),
    )
}

#[test]
fn an_unknown_rule_id_is_dropped_and_the_finding_kept() {
    let f = Fixture::new();
    let reviewer = StubReviewer::new().reply(
        &["orders.py"],
        &reply(
            "convention",
            2,
            "cancel() ignores the rule.",
            "Tabs, never spaces",
        ),
    );
    ran(f.run(&reviewer, &StubDecider::new(), false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.rule, None);
    assert_eq!(g.statement, "cancel() ignores the rule.");
}

#[test]
fn a_rule_cited_by_id_is_kept_and_jev_sees_its_text() {
    let f = Fixture::new();
    let reviewer = StubReviewer::new().reply(
        &["orders.py"],
        &reply("convention", 2, "cancel() panics.", "`Result`"),
    );
    let decider = StubDecider::new();
    ran(f.run(&reviewer, &decider, false));
    assert_eq!(only(&f.store()).rule.as_deref(), Some("result"));
    let q = decider
        .asked()
        .into_iter()
        .find(|q| q.kind == "nitpick")
        .unwrap();
    assert_eq!(
        StubDecider::context(&q, "project rule"),
        "Commands return a Result\nA command never panics; it returns an error."
    );
    let prompt = scry::domain::review_prompt::review_prompt(&reviewer.asks()[0]);
    assert!(
        prompt.contains("- result: Commands return a Result"),
        "{prompt}"
    );
}

#[test]
fn a_rule_text_citation_is_not_resolved_by_substring() {
    let f = Fixture::new();
    for cited in ["clock", "Commands return a Result"] {
        let reviewer = StubReviewer::new().reply(
            &["orders.py"],
            &reply("convention", 2, "cancel() reads the time.", cited),
        );
        ran(f.run(&reviewer, &StubDecider::new(), true));
    }
    let s = f.store();
    assert!(
        s.findings.iter().all(|g| g.rule.is_none()),
        "{:#?}",
        s.findings
    );
}

#[test]
fn the_same_problem_merges_into_the_stored_id() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let id = only(&f.store()).id.clone();
    let decider = StubDecider::new().on("same", |_| true, StubDecider::p_yes(0.8));
    let reviewer = bug_reviewer("cancel() gives back None, not the order.");
    let r = ran(f.run(&reviewer, &decider, true));
    assert!(r.new.is_empty());
    assert_eq!(reviewer.asks()[0].open.len(), 1);
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.id, id);
    assert_eq!(g.seen, 2);
    assert_eq!(g.statement, "cancel() gives back None, not the order.");
    assert_eq!(g.status, Status::Unverified);
    let kinds: Vec<String> = decider.asked().into_iter().map(|q| q.kind).collect();
    assert_eq!(kinds, ["same"]);
}

#[test]
fn a_duplicate_within_one_pass_keeps_the_first_wording() {
    let f = Fixture::new();
    let finding = |line: u32, statement: &str| {
        serde_json::json!({
            "issue_type": "bug",
            "file": "orders.py",
            "lines": [line, line],
            "statement": statement,
            "rule": "",
            "evidence": [{"file": "orders.py", "lines": [line, line]}],
        })
    };
    let reply = serde_json::json!({
        "findings": [
            finding(2, "cancel() returns None."),
            finding(1, "cancel() gives back None, not the order."),
        ],
        "open": [],
    })
    .to_string();
    let reviewer = StubReviewer::new().reply(&["orders.py"], &reply);
    let decider = StubDecider::new().on("same", |_| true, StubDecider::p_yes(0.8));
    let r = ran(f.run(&reviewer, &decider, false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(r.new, std::slice::from_ref(&g.id));
    assert_eq!(g.seen, 2);
    assert_eq!(g.statement, "cancel() returns None.");
    assert_eq!(g.place.lines.start, 2);
    assert_eq!(g.evidence[0].lines.start, 2);
}

#[test]
fn dismissed_everywhere_drops_the_new_finding() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let id = only(&f.store()).id.clone();
    support::update_store(&f.project, |s| {
        s.dismiss(&id.0, Dismissal::Everywhere, t0())
    })
    .unwrap()
    .unwrap();
    let decider = StubDecider::new().on("same", |_| true, StubDecider::p_yes(0.8));
    let r = ran(f.run(
        &bug_reviewer("cancel() returns None again."),
        &decider,
        true,
    ));
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
        &bug_reviewer("cancel() returns None."),
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
    let reviewer = StubReviewer::new().reply(&["orders.py"], &answer);
    ran(f.run(&reviewer, &StubDecider::new(), false));
    assert_eq!(reviewer.asks()[0].open[0].id, id);
    assert_eq!(only(&f.store()).status, Status::Resolved);
}

#[test]
fn still_holds_keeps_the_finding() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let id = only(&f.store()).id.clone();
    // edited without a fix: a new fingerprint, and the reviewer may not raise it again
    f.edit("cancel(now=True)", "cancel(now=True)  # log");
    let answer = serde_json::json!({
        "findings": [],
        "open": [{"id": id.0, "holds": "still holds", "why": "it still returns None"}],
    })
    .to_string();
    let reviewer = StubReviewer::new().reply(&["orders.py"], &answer);
    let r = ran(f.run(&reviewer, &StubDecider::new(), false));
    assert!(r.whole);
    assert_eq!(reviewer.asks()[0].open[0].id, id);
    let s = f.store();
    assert_eq!(only(&s).status, Status::Unverified);
    assert!(s.reviewed[Path::new("orders.py")].contains(&cancel_print(&f)));
}

#[test]
fn no_longer_holds_resolves_only_a_finding_the_call_was_shown() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    // an unverified finding on `place`, which this pass does not send, raised on
    // `place` as it is (so it is not a unit put back)
    let place_now = f
        .view
        .units(Path::new("orders.py"))
        .unwrap()
        .into_iter()
        .find(|u| u.name == "place")
        .unwrap()
        .fingerprint;
    support::update_store(&f.project, |s| {
        let mut other = s.findings[0].clone();
        other.id = scry::domain::finding::FindingId("abcdef".into());
        other.place.unit = Some("place".into());
        other.place.fingerprint = Some(place_now);
        s.findings.push(other);
    })
    .unwrap();
    f.edit("cancel(now=True)", "cancel(now=True) or order");
    let answer = serde_json::json!({
        "findings": [],
        "open": [{"id": "abcdef", "holds": "no longer holds", "why": "a wrong id"}],
    })
    .to_string();
    let reviewer = StubReviewer::new().reply(&["orders.py"], &answer);
    ran(f.run(&reviewer, &StubDecider::new(), false));
    assert!(reviewer.asks()[0].open.iter().all(|o| o.id.0 != "abcdef"));
    let s = f.store();
    let other = s.findings.iter().find(|g| g.id.0 == "abcdef").unwrap();
    assert_eq!(other.status, Status::Unverified);
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
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    assert_eq!(only(&f.store()).status, Status::Unverified);

    // cancel changes again and big too; big's call fails: nothing resolves
    f.edit("cancel(now=False)", "cancel(now=True)");
    write(f.root(), "big.py", &big.replace("x = 1", "x = 2"));
    let failing = StubReviewer::new().reply(&["big.py"], "I could not finish.");
    let r = ran(f.run(&failing, &StubDecider::new(), false));
    assert_eq!(failing.asks().len(), 2);
    assert!(!r.whole);
    assert_eq!(r.failures.len(), 1);
    assert_eq!(only(&f.store()).status, Status::Unverified);

    // the same tree again, every call answering: not found again, resolved
    let r = ran(f.run(&StubReviewer::new(), &StubDecider::new(), true));
    assert!(r.whole);
    assert_eq!(only(&f.store()).status, Status::Resolved);
}

#[test]
fn a_unit_removed_from_the_file_makes_its_findings_gone() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    std::fs::write(
        f.root().join("orders.py"),
        "def place(order):\n    return order.place()\n",
    )
    .unwrap();
    ran(f.run(&StubReviewer::new(), &StubDecider::new(), false));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.status, Status::Gone);
    assert_eq!(g.history.last().unwrap().1, Status::Gone);
}

#[test]
fn a_review_reply_with_no_json_is_a_failure_and_leaves_units_unreviewed() {
    let f = Fixture::new();
    let reviewer = StubReviewer::new().reply(&["orders.py"], "Nothing to report, I think.");
    let r = ran(f.run(&reviewer, &StubDecider::new(), false));
    assert_eq!(r.failures.len(), 1);
    assert!(r.failures[0].contains("orders.py"), "{:?}", r.failures);
    assert!(r.failures[0].contains("no JSON"), "{:?}", r.failures);
    assert!(!r.whole);
    assert_eq!(r.looked_at, 0);
    assert!(f.store().reviewed.is_empty());
    ran(f.run(&reviewer, &StubDecider::new(), false));
    assert_eq!(reviewer.asks().len(), 2);
}

#[test]
fn jev_silent_skips_and_writes_nothing() {
    let f = Fixture::new();
    ran(f.run(&StubReviewer::new(), &StubDecider::new(), false));
    let before = std::fs::read(f.store_path()).unwrap();
    let decider = StubDecider::new().on("nitpick", |_| true, Answer::Unsure);
    match f.run(&bug_reviewer("cancel() returns None."), &decider, true) {
        ReviewOutcome::Skipped(why) => assert_eq!(why, "Jev did not answer"),
        ReviewOutcome::Ran(r) => panic!("ran: {r:?}"),
    }
    assert_eq!(std::fs::read(f.store_path()).unwrap(), before);
}

#[test]
fn a_dismissal_during_the_pass_stands() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let id = only(&f.store()).id.clone();
    let project = f.project.clone();
    let dismissed = id.clone();
    let reviewer = bug_reviewer("cancel() gives back None.").on_review(move |_| {
        support::update_store(&project, |s| s.dismiss(&dismissed.0, Dismissal::Here, t0()))
            .unwrap()
            .unwrap();
    });
    let decider = StubDecider::new().on("same", |_| true, StubDecider::p_yes(0.8));
    ran(f.run(&reviewer, &decider, true));
    let s = f.store();
    let g = only(&s);
    assert_eq!(g.id, id);
    assert_eq!(g.status, Status::DismissedHere);
    assert!(!s.reviewed.is_empty());
}

#[test]
fn an_unreadable_store_stops_the_pass_untouched() {
    let f = Fixture::new();
    let path = f.store_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = br#"{"version": 2, "data": {}}"#;
    std::fs::write(&path, bytes).unwrap();
    let reviewer = StubReviewer::new();
    let e = f
        .try_run(&reviewer, &StubDecider::new(), false)
        .unwrap_err()
        .to_string();
    assert!(e.contains("version 2"), "{e}");
    assert!(reviewer.asks().is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

/// A new finding whose id another writer took meanwhile is written under a
/// fresh id, not dropped; a later write of the same pass finds it there.
#[test]
fn a_new_finding_whose_id_was_taken_meanwhile_is_re_ided() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    let ours = only(&f.store()).clone();
    let mut theirs = ours.clone();
    theirs.statement = "added from another terminal".into();
    let base = Findings::default();
    let mine = Findings {
        findings: vec![ours.clone()],
        ..Findings::default()
    };
    let current = Findings {
        findings: vec![theirs.clone()],
        ..Findings::default()
    };
    let mut renames = std::collections::HashMap::new();
    let written = merge_into(&base, &mine, &current, &mut renames);
    assert_eq!(written.findings.len(), 2, "{:#?}", written.findings);
    assert_eq!(written.findings[0], theirs);
    let to = renames[&ours.id].clone();
    assert_ne!(to, ours.id);
    assert_eq!(written.findings[1].id, to);
    assert_eq!(written.findings[1].statement, ours.statement);

    // the pass goes on under its own id; the next write lands on the new one
    let mut later = mine.clone();
    later.findings[0].set_status(Status::Open, t0());
    let again = merge_into(&renamed(&mine, &renames), &later, &written, &mut renames);
    assert_eq!(again.findings.len(), 2, "{:#?}", again.findings);
    assert_eq!(again.findings[0], theirs);
    assert_eq!(again.findings[1].id, to);
    assert_eq!(again.findings[1].status, Status::Open);
}

// ---- the trace ----

/// A pass with the doubles, a claim check that could not confirm one of
/// its citations, and a file no language covers: its trace and the scope's
/// say what was sent, what came back and what each step decided.
#[test]
fn a_pass_traces_its_prompt_reply_judgements_and_checks() {
    use sorcerer_models::domain::claim::{CitationCheck, Cited, RawCitation, Verdict};
    use std::cell::RefCell;

    let f = Fixture::new();
    write(f.root(), "notes.txt", "not code\n");
    let reviewer = bug_reviewer("cancel ignores the order's state");
    let decider = StubDecider::new().on("nitpick", |_| true, StubDecider::p_yes(0.2));
    let checker = |_: &Claim| -> sorcerer_models::Result<Checked> {
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
            escalation: Escalation::ToTheStrongModel,
            errands: vec![
                support::errand("check", "haiku", 0.01),
                support::errand("check", "sonnet", 0.03),
            ],
        })
    };
    let log = RefCell::new(String::new());
    let trace = |s: &str| log.borrow_mut().push_str(s);
    let clock = FixedClock::at(t0());
    let progress = |_: &Progress| {};
    let ports = support::Ports::of(&f.project);
    let cancel = Cancel::default();
    let deps = ReviewDeps {
        view: &f.view,
        reviewer: &reviewer,
        decider: &decider,
        checker: &checker,
        store: &ports.store,
        ledger: &ports.ledger,
        settings: &ports.settings,
        clock: &clock,
        log: &trace,
        progress: &progress,
        cancel: &cancel,
        review_cancel: &cancel,
    };
    let req = ReviewRequest {
        caps: Some(Caps {
            pass: None,
            day: f64::INFINITY,
            budget: None,
        }),
        ..ReviewRequest::of(ScopeRequest::Working)
    };
    let r = ran(review_change(&f.project, &req, &deps).unwrap());
    assert_eq!(r.new.len(), 1);
    assert_eq!(r.touched, r.new);
    let log = log.into_inner();
    let has = |s: &str| assert!(log.contains(s), "no {s:?} in the trace:\n{log}");
    has("`notes.txt`: skipped (no language)");
    has("## Review calls");
    has("## Reviewed");
    has(&scry::domain::review_prompt::review_prompt(
        &reviewer.asks()[0],
    ));
    has(&reply("bug", 2, "cancel ignores the order's state", ""));
    has("nitpick? P(yes) 0.20 → confidence 0.80, threshold 0.70 → unverified");
    has(&format!("id {}", r.new[0].0));
    has("cannot_tell");
    has("escalated: yes");
    has("models: haiku, sonnet");
    has("`orders.py` `cancel` line 2: confirmed");
    has("`orders.py` `Order.refund`: not found");
    has("→ unsure");
    // the stored verdict keeps what the check cited
    let s = f.store();
    let v = only(&s).verdict.clone().unwrap();
    assert_eq!(v.cited.len(), 2);
    assert!(!v.cited[1].confirmed && v.cited[1].unit.as_deref() == Some("Order.refund"));
}

/// A unit of N lines named NAME: its definition and N - 1 numbered lines.
fn long_unit(name: &str, n: u32) -> String {
    let mut text = format!("def {name}():\n");
    for i in 1..n {
        text.push_str(&format!("    x{i} = {i}\n"));
    }
    text
}

/// What a pass gave: its outcome, its run log and its progress events.
struct Logged {
    outcome: sorcerer_core::Result<ReviewOutcome>,
    log: String,
    progress: Vec<Progress>,
}

/// One pass over the working tree of PROJECT under a pass cap of PASS_CAP
/// (none: unbounded), the claim checks failing, CANCEL as given.
fn pass_logged(
    project: &Arc<Project>,
    view: &FilesView,
    reviewer: &StubReviewer,
    pass_cap: Option<f64>,
    cancel: &Cancel,
) -> Logged {
    use std::sync::Mutex;
    let checker = |_: &Claim| -> sorcerer_models::Result<Checked> {
        Err(sorcerer_models::Error::Reply {
            provider: "stub".into(),
            why: "no claim check in these tests".into(),
        })
    };
    let log = Mutex::new(String::new());
    let trace = |s: &str| log.lock().unwrap().push_str(s);
    let progress = Mutex::new(Vec::new());
    let on_progress = |p: &Progress| progress.lock().unwrap().push(p.clone());
    let clock = FixedClock::at(t0());
    let ports = support::Ports::of(project);
    let decider = StubDecider::new();
    let deps = ReviewDeps {
        view,
        reviewer,
        decider: &decider,
        checker: &checker,
        store: &ports.store,
        ledger: &ports.ledger,
        settings: &ports.settings,
        clock: &clock,
        log: &trace,
        progress: &on_progress,
        cancel,
        review_cancel: cancel,
    };
    let req = ReviewRequest {
        caps: Some(Caps {
            pass: pass_cap,
            day: f64::INFINITY,
            budget: None,
        }),
        ..ReviewRequest::of(ScopeRequest::Working)
    };
    let outcome = review_change(project, &req, &deps);
    Logged {
        outcome,
        log: log.into_inner().unwrap(),
        progress: progress.into_inner().unwrap(),
    }
}

/// The run log from its `Units` heading to the end of its `Review calls`
/// section.
fn review_calls_text(log: &str) -> &str {
    let start = log.find("\n## Units\n").expect("a Units section");
    let calls = start
        + log[start..]
            .find("\n## Review calls\n")
            .expect("a Review calls section");
    let end = log[calls + 1..]
        .find("\n## ")
        .map_or(log.len(), |e| calls + 1 + e);
    &log[start..end]
}

/// Three files of one unit each (400, 400 and 300 lines, so one batch
/// each) and a small one, committed, then each changed.
fn three_batches() -> Fixture {
    let dir = git_project();
    write(dir.path(), ".sorcerer/scry.toml", STEERING);
    write(dir.path(), "a.py", &long_unit("alpha", 400));
    write(dir.path(), "b.py", &long_unit("beta", 400));
    write(dir.path(), "c.py", &long_unit("gamma", 300));
    write(dir.path(), "d.py", "def delta():\n    return 1\n");
    commit(dir.path(), "four files");
    for (file, from) in [
        ("a.py", "x1 = 1"),
        ("b.py", "x1 = 1"),
        ("c.py", "x1 = 1"),
        ("d.py", "return 1"),
    ] {
        let p = dir.path().join(file);
        let text = std::fs::read_to_string(&p).unwrap();
        std::fs::write(p, text.replacen(from, &format!("{from} + 0"), 1)).unwrap();
    }
    let (project, view) = support::project_with_vendored_grammars(dir.path());
    Fixture { dir, project, view }
}

/// Changes the second line of each of the three long files, so their units
/// are sent again (d.py's is left as reviewed).
fn change_the_long_units_again(f: &Fixture) {
    for file in ["a.py", "b.py", "c.py"] {
        let p = f.root().join(file);
        let text = std::fs::read_to_string(&p).unwrap();
        std::fs::write(p, text.replacen("x1 = 1 + 0", "x1 = 1 + 1", 1)).unwrap();
    }
}

fn reply_on(file: &str, line: u32, statement: &str, open: serde_json::Value) -> String {
    serde_json::json!({
        "findings": [{
            "issue_type": "bug",
            "file": file,
            "lines": [line, line],
            "statement": statement,
            "rule": "",
            "evidence": [{"file": file, "lines": [line, line]}],
        }],
        "open": open,
    })
    .to_string()
}

#[test]
fn the_run_log_of_the_review_calls_is_as_recorded() {
    let f = three_batches();
    // the first pass leaves an unverified finding on alpha and every unit reviewed
    let first = StubReviewer::new().reply(
        &["a.py"],
        &reply_on(
            "a.py",
            2,
            "alpha's first line is off by one",
            serde_json::json!([]),
        ),
    );
    ran(
        pass_logged(&f.project, &f.view, &first, None, &Cancel::default())
            .outcome
            .unwrap(),
    );
    let open_id = only(&f.store()).id.0.clone();
    change_the_long_units_again(&f);

    let answers = serde_json::json!([
        {"id": open_id, "holds": "still holds", "why": "unchanged"},
        {"id": "zzzzzz", "holds": "no longer holds", "why": "not shown"},
    ]);
    let reviewer = StubReviewer::new()
        .reply(
            &["a.py"],
            &reply_on("a.py", 3, "alpha's second line is off by one", answers),
        )
        .reply(&["b.py"], "I could not find anything to say in JSON.");
    // room for two calls at $0.04, not a third
    let pass = pass_logged(
        &f.project,
        &f.view,
        &reviewer,
        Some(0.10),
        &Cancel::default(),
    );
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/run-log-send.md");
    let text = review_calls_text(&pass.log);
    if std::env::var_os("SCRY_BLESS").is_some() {
        std::fs::write(&golden_path, text).unwrap();
    }
    let golden = std::fs::read_to_string(&golden_path).unwrap();
    assert_eq!(
        text, golden,
        "the run log drifted; SCRY_BLESS=1 rewrites the golden file"
    );

    let r = ran(pass.outcome.unwrap());
    assert_eq!(r.capped, 1);
    assert_eq!(r.failures.len(), 1);
    assert!(
        r.failures[0].starts_with("review failed for b.py: "),
        "{:?}",
        r.failures
    );
    assert!(!r.whole);
    assert_eq!(reviewer.asks().len(), 2);
    let s = f.store();
    let units_of =
        |m: &std::collections::BTreeMap<PathBuf, std::collections::BTreeSet<String>>,
         file: &str| m.get(Path::new(file)).cloned().unwrap_or_default();
    assert_eq!(
        units_of(&s.capped, "c.py").into_iter().collect::<Vec<_>>(),
        ["gamma"]
    );
    assert!(units_of(&s.capped, "a.py").is_empty() && units_of(&s.capped, "b.py").is_empty());
    let alpha_now = f
        .view
        .units(Path::new("a.py"))
        .unwrap()
        .into_iter()
        .find(|u| u.name == "alpha")
        .unwrap()
        .fingerprint
        .to_string();
    assert!(units_of(&s.reviewed, "a.py").contains(&alpha_now));
    assert!(matches!(pass.progress.first(), Some(Progress::Sending(_))));
    assert_eq!(pass.progress.get(1), Some(&Progress::Reviewed(1)));
    assert_eq!(
        pass.progress
            .iter()
            .filter(|p| matches!(p, Progress::Reviewed(_)))
            .count(),
        1
    );
    assert_eq!(pass.progress.last(), Some(&Progress::Stored));
}

#[test]
fn after_one_batch_is_held_back_every_later_one_is() {
    let f = three_batches();
    let reviewer = StubReviewer::new();
    // room for one call at $0.04, not two
    let pass = pass_logged(
        &f.project,
        &f.view,
        &reviewer,
        Some(0.05),
        &Cancel::default(),
    );
    let r = ran(pass.outcome.unwrap());
    assert_eq!(reviewer.asks().len(), 1);
    assert_eq!(
        r.capped, 3,
        "b.py's unit, and c.py's and d.py's batched together"
    );
    let s = f.store();
    let capped: Vec<String> = s
        .capped
        .iter()
        .flat_map(|(f, us)| us.iter().map(move |u| format!("{}:{u}", f.display())))
        .collect();
    assert_eq!(capped, ["b.py:beta", "c.py:gamma", "d.py:delta"]);
    assert_eq!(
        pass.log.matches("these units wait for a later run").count(),
        2
    );
}

#[test]
fn a_cancel_before_the_pass_fails_every_batch_and_asks_nothing() {
    let f = three_batches();
    let reviewer = StubReviewer::new();
    let cancel = Cancel::default();
    cancel.cancel();
    let pass = pass_logged(&f.project, &f.view, &reviewer, None, &cancel);
    let r = ran(pass.outcome.unwrap());
    assert!(reviewer.asks().is_empty());
    assert_eq!(r.capped, 0);
    assert_eq!(
        r.failures,
        [
            "review cancelled for a.py",
            "review cancelled for b.py",
            "review cancelled for c.py, d.py",
        ]
    );
    assert_eq!(pass.log.matches("- cancelled before the call\n").count(), 3);
    assert!(f.store().capped.is_empty());
}

// ---- the findings' run log, Jev's later silences, the mid-round write ----

/// One pass over PROJECT's working tree, every unit changed sent again,
/// with these doubles and STORE; no cap.
fn pass_with(
    project: &Arc<Project>,
    view: &FilesView,
    reviewer: &StubReviewer,
    decider: &StubDecider,
    checker: &dyn scry::ports::claim_checker::ClaimChecker,
    store: &dyn FindingStore,
) -> Logged {
    use std::sync::Mutex;
    let log = Mutex::new(String::new());
    let trace = |s: &str| log.lock().unwrap().push_str(s);
    let progress = Mutex::new(Vec::new());
    let on_progress = |p: &Progress| progress.lock().unwrap().push(p.clone());
    let clock = FixedClock::at(t0());
    let ports = support::Ports::of(project);
    let cancel = Cancel::default();
    let deps = ReviewDeps {
        view,
        reviewer,
        decider,
        checker,
        store,
        ledger: &ports.ledger,
        settings: &ports.settings,
        clock: &clock,
        log: &trace,
        progress: &on_progress,
        cancel: &cancel,
        review_cancel: &cancel,
    };
    let req = ReviewRequest {
        fresh: true,
        caps: Some(Caps {
            pass: None,
            day: f64::INFINITY,
            budget: None,
        }),
        ..ReviewRequest::of(ScopeRequest::Working)
    };
    let outcome = review_change(project, &req, &deps);
    Logged {
        outcome,
        log: log.into_inner().unwrap(),
        progress: progress.into_inner().unwrap(),
    }
}

/// A claim check that fails: an unverified finding stays unverified.
fn failing_check(_: &Claim) -> sorcerer_models::Result<Checked> {
    Err(sorcerer_models::Error::Reply {
        provider: "stub".into(),
        why: "no claim check in these tests".into(),
    })
}

/// The run log from its `FIRST` heading to the end of its `LAST` section.
fn sections_text<'l>(log: &'l str, first: &str, last: &str) -> &'l str {
    let start = log
        .find(&format!("\n## {first}"))
        .unwrap_or_else(|| panic!("no {first} section in:\n{log}"));
    let at = start
        + log[start..]
            .find(&format!("\n## {last}"))
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

/// A review reply of FINDINGS, each (file, line, statement, rule), and OPEN.
fn reply_of(findings: &[(&str, u32, &str, &str)], open: serde_json::Value) -> String {
    let findings: Vec<serde_json::Value> = findings
        .iter()
        .map(|(file, line, statement, rule)| {
            serde_json::json!({
                "issue_type": "bug",
                "file": file,
                "lines": [line, line],
                "statement": statement,
                "rule": rule,
                "evidence": [{"file": file, "lines": [line, line]}],
            })
        })
        .collect();
    serde_json::json!({"findings": findings, "open": open}).to_string()
}

/// Jev says yes to the same-problem question only for these pairs of
/// (new statement, stored statement); NITPICKS answer the nitpick question
/// by statement.
fn judging_decider(pairs: &[(&str, &str)], nitpicks: &[(&str, Answer)]) -> StubDecider {
    let mut d = StubDecider::new();
    for (new, stored) in pairs {
        let (new, stored) = (new.to_string(), stored.to_string());
        d = d.on(
            "same",
            move |q| {
                StubDecider::context(q, "finding A").ends_with(&new)
                    && StubDecider::context(q, "finding B").ends_with(&stored)
            },
            StubDecider::p_yes(0.9),
        );
    }
    for (statement, answer) in nitpicks {
        let statement = statement.to_string();
        d = d.on(
            "nitpick",
            move |q| StubDecider::context(q, "finding") == statement,
            answer.clone(),
        );
    }
    d
}

const MANY_A: &str = "def alpha(x):\n    return x + 1\n\n\ndef beta(x):\n    return x * 2\n";
const MANY_B: &str = "def gamma(x):\n    return x - 1\n\n\ndef delta(x):\n    return x / 2\n";
const MANY_C: &str = "def eps(x):\n    return round(x)\n";
const MANY_D: &str = "def zeta(x):\n    return x ** 2\n";
const MANY_E: &str = "def iota(x):\n    return 1 / x\n";

/// Five small files committed, then every unit changed once (`+ 0`), and
/// a first pass that stored one finding per unit, each then set to the
/// status the second pass starts from.
fn many_findings() -> Fixture {
    let dir = git_project();
    write(dir.path(), ".sorcerer/scry.toml", STEERING);
    let files = [
        ("a.py", MANY_A),
        ("b.py", MANY_B),
        ("c.py", MANY_C),
        ("d.py", MANY_D),
        ("e.py", MANY_E),
    ];
    for (file, text) in files {
        write(dir.path(), file, text);
    }
    commit(dir.path(), "five files");
    for (file, text) in files {
        write(dir.path(), file, &text.replace("\n\n\n", " + 0\n\n\n"));
        let p = dir.path().join(file);
        let t = std::fs::read_to_string(&p).unwrap();
        if !t.ends_with(" + 0\n") {
            std::fs::write(p, t.trim_end().to_string() + " + 0\n").unwrap();
        }
    }
    let (project, view) = support::project_with_vendored_grammars(dir.path());
    let f = Fixture { dir, project, view };
    let seed = StubReviewer::new().reply(
        &["a.py", "b.py", "c.py", "d.py", "e.py"],
        &reply_of(
            &[
                ("a.py", 2, "alpha adds one", ""),
                ("b.py", 6, "delta drops the remainder", ""),
                ("c.py", 2, "eps rounds half to even", ""),
                ("d.py", 2, "zeta overflows", ""),
                ("a.py", 6, "beta doubles twice", ""),
                ("b.py", 2, "gamma leaks", ""),
                ("e.py", 2, "iota divides by zero", ""),
            ],
            serde_json::json!([]),
        ),
    );
    let store = support::store_of(&f.project);
    let first = pass_with(
        &f.project,
        &f.view,
        &seed,
        &StubDecider::new(),
        &failing_check,
        &store,
    );
    ran(first.outcome.unwrap());
    support::update_store(&f.project, |s| {
        for g in &mut s.findings {
            match g.statement.as_str() {
                "alpha adds one" | "beta doubles twice" | "gamma leaks" => {
                    g.set_status(Status::Open, t0());
                }
                "eps rounds half to even" => {
                    g.set_status(Status::Withheld, t0());
                    g.scores.asked_on = Some("what it was asked on before".into());
                }
                "zeta overflows" => {
                    g.set_status(Status::Refuted, t0());
                    for e in &mut g.evidence {
                        e.print = Some("an older print".into());
                    }
                }
                "iota divides by zero" => g.set_status(Status::DismissedHere, t0()),
                _ => {}
            }
        }
    })
    .unwrap();
    f
}

/// The statuses of STORE's findings, by statement.
fn statuses(s: &Findings) -> Vec<(String, Status)> {
    s.findings
        .iter()
        .map(|g| (g.statement.clone(), g.status))
        .collect()
}

#[test]
fn the_run_log_of_the_findings_is_as_recorded() {
    let f = many_findings();
    let alpha_id = f
        .store()
        .findings
        .iter()
        .find(|g| g.statement == "alpha adds one")
        .unwrap()
        .id
        .0
        .clone();
    // every unit changed again; delta removed from b.py
    for (file, text) in [
        ("a.py", MANY_A),
        ("b.py", MANY_B),
        ("c.py", MANY_C),
        ("d.py", MANY_D),
        ("e.py", MANY_E),
    ] {
        let text = text.replace("\n\n\n", " + 1\n\n\n");
        let text = text.trim_end().to_string() + " + 1\n";
        let text = if file == "b.py" {
            "def gamma(x):\n    return x - 1 + 1\n".to_string()
        } else {
            text
        };
        write(f.root(), file, &text);
    }
    let answers = serde_json::json!([
        {"id": alpha_id, "holds": "no longer holds", "why": "it adds two now"},
    ]);
    let reviewer = StubReviewer::new().reply(
        &["a.py", "b.py", "c.py", "d.py", "e.py"],
        &reply_of(
            &[
                ("a.py", 3, "a stray blank line", ""),
                ("a.py", 2, "alpha ignores negatives", "no-such-rule"),
                ("b.py", 2, "gamma leaks a handle", ""),
                ("e.py", 2, "iota may divide by zero", ""),
                ("a.py", 5, "beta has a long name", ""),
                ("b.py", 2, "gamma never closes its file", ""),
                ("c.py", 2, "eps rounds half to even again", ""),
                ("d.py", 2, "zeta overflows on big input", ""),
            ],
            answers,
        ),
    );
    let decider = judging_decider(
        &[
            ("gamma leaks a handle", "gamma leaks"),
            ("iota may divide by zero", "iota divides by zero"),
            ("eps rounds half to even again", "eps rounds half to even"),
            ("zeta overflows on big input", "zeta overflows"),
        ],
        &[
            ("beta has a long name", StubDecider::p_yes(0.9)),
            ("gamma never closes its file", StubDecider::p_yes(0.2)),
            ("eps rounds half to even again", StubDecider::p_yes(0.1)),
        ],
    );
    let store = support::store_of(&f.project);
    let pass = pass_with(
        &f.project,
        &f.view,
        &reviewer,
        &decider,
        &failing_check,
        &store,
    );
    assert_golden(
        "run-log-findings.md",
        sections_text(&pass.log, "Findings", "Reviewed"),
    );
    let r = ran(pass.outcome.unwrap());
    assert!(r.whole);
    let st = |s: &str| (s.to_string(), Status::Unverified);
    let mut want = vec![
        ("alpha adds one".to_string(), Status::Resolved),
        ("delta drops the remainder".to_string(), Status::Gone),
        st("eps rounds half to even again"),
        st("zeta overflows on big input"),
        ("beta doubles twice".to_string(), Status::Resolved),
        ("gamma leaks a handle".to_string(), Status::Open),
        ("iota divides by zero".to_string(), Status::DismissedHere),
        ("a stray blank line".to_string(), Status::Elsewhere),
        st("alpha ignores negatives"),
        ("beta has a long name".to_string(), Status::Withheld),
        st("gamma never closes its file"),
    ];
    let mut got = statuses(&f.store());
    want.sort_by(|a, b| a.0.cmp(&b.0));
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got, want);
}

/// A pass over F whose Jev is DECIDER is skipped for Jev's silence, leaves
/// the store's file as it was and tells no `Stored`.
fn assert_silent_pass_writes_nothing(f: &Fixture, reviewer: &StubReviewer, decider: &StubDecider) {
    let before = std::fs::read(f.store_path()).unwrap();
    let store = support::store_of(&f.project);
    let pass = pass_with(
        &f.project,
        &f.view,
        reviewer,
        decider,
        &failing_check,
        &store,
    );
    match pass.outcome.unwrap() {
        ReviewOutcome::Skipped(why) => assert_eq!(why, "Jev did not answer"),
        ReviewOutcome::Ran(r) => panic!("ran: {r:?}"),
    }
    assert_eq!(std::fs::read(f.store_path()).unwrap(), before);
    assert!(
        !pass.progress.contains(&Progress::Stored),
        "{:?}",
        pass.progress
    );
}

#[test]
fn jev_silent_at_the_nitpick_question_writes_nothing() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    // the same-problem question is answered (not the same); the nitpick not
    let decider = StubDecider::new().on("nitpick", |_| true, Answer::Unsure);
    assert_silent_pass_writes_nothing(&f, &bug_reviewer("cancel() ignores now."), &decider);
    let kinds: Vec<String> = decider.asked().into_iter().map(|q| q.kind).collect();
    assert_eq!(kinds, ["same", "nitpick"]);
}

#[test]
fn jev_silent_at_a_withheld_merges_question_writes_nothing() {
    let f = Fixture::new();
    ran(f.run(
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        false,
    ));
    support::update_store(&f.project, |s| {
        let g = &mut s.findings[0];
        g.set_status(Status::Withheld, t0());
        g.scores.asked_on = Some("what it was asked on before".into());
    })
    .unwrap();
    let decider = StubDecider::new()
        .on("same", |_| true, StubDecider::p_yes(0.9))
        .on("nitpick", |_| true, Answer::Unsure);
    assert_silent_pass_writes_nothing(&f, &bug_reviewer("cancel() gives back None."), &decider);
    let kinds: Vec<String> = decider.asked().into_iter().map(|q| q.kind).collect();
    assert_eq!(kinds, ["same", "nitpick"]);
}

/// The project's store, but before the pass's first write another writer
/// takes the id the pass gives its new finding.
struct TakenBeforeTheFirstWrite {
    inner: scry::adapters::json_finding_store::JsonFindingStore,
    taken: std::sync::Mutex<Option<scry::domain::finding::FindingId>>,
}

impl FindingStore for TakenBeforeTheFirstWrite {
    fn read(&self) -> sorcerer_core::Result<Snapshot<Findings>> {
        self.inner.read()
    }

    fn update(&self, f: &mut dyn FnMut(&mut Findings)) -> sorcerer_core::Result<()> {
        let mut taken = self.taken.lock().unwrap();
        self.inner.update(&mut |current| {
            if taken.is_none() {
                // what the pass would write: its new finding's id, taken first
                let mut preview = current.clone();
                f(&mut preview);
                let ours = preview
                    .findings
                    .iter()
                    .find(|g| !current.findings.iter().any(|c| c.id == g.id))
                    .expect("the pass writes a new finding")
                    .clone();
                let mut theirs = ours.clone();
                theirs.statement = "added from another terminal".into();
                current.findings.push(theirs);
                *taken = Some(ours.id);
            }
            f(current);
        })
    }

    fn hold(&self) -> sorcerer_core::Result<Box<dyn scry::ports::finding_store::Held>> {
        self.inner.hold()
    }

    fn path(&self) -> &Path {
        self.inner.path()
    }
}

#[test]
fn an_id_taken_before_the_mid_round_write_stays_renamed_at_the_end() {
    use sorcerer_models::domain::claim::Verdict;
    let f = Fixture::new();
    let right = |_: &Claim| -> sorcerer_models::Result<Checked> {
        Ok(Checked {
            verdict: Verdict::Right,
            why: "it does".into(),
            citations: Vec::new(),
            cited: Vec::new(),
            escalation: Escalation::No,
            errands: vec![support::errand("check", "haiku", 0.01)],
        })
    };
    let store = TakenBeforeTheFirstWrite {
        inner: support::store_of(&f.project),
        taken: std::sync::Mutex::new(None),
    };
    let pass = pass_with(
        &f.project,
        &f.view,
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
        &right,
        &store,
    );
    let r = ran(pass.outcome.unwrap());
    let taken = store
        .taken
        .lock()
        .unwrap()
        .clone()
        .expect("a write happened");
    // the mid-round write came before the round's check
    assert_eq!(
        pass.progress
            .iter()
            .filter(|p| **p == Progress::Stored)
            .count(),
        2,
        "{:?}",
        pass.progress
    );
    let s = f.store();
    assert_eq!(s.findings.len(), 2, "{:#?}", s.findings);
    let theirs = s.findings.iter().find(|g| g.id == taken).unwrap();
    assert_eq!(theirs.statement, "added from another terminal");
    assert!(theirs.verdict.is_none());
    let ours: Vec<&Finding> = s
        .findings
        .iter()
        .filter(|g| g.statement == "cancel() returns None.")
        .collect();
    assert_eq!(ours.len(), 1);
    assert_ne!(ours[0].id, taken);
    assert!(ours[0].verdict.is_some());
    assert_eq!(r.new, std::slice::from_ref(&ours[0].id));
    assert!(r.touched.contains(&ours[0].id), "{:?}", r.touched);
    assert!(!r.touched.contains(&taken), "{:?}", r.touched);
}

/// The run log of one pass over F (every changed unit sent again) with
/// REVIEWER and DECIDER, the claim checks failing.
fn log_of(f: &Fixture, reviewer: &StubReviewer, decider: &StubDecider) -> String {
    let store = support::store_of(&f.project);
    pass_with(
        &f.project,
        &f.view,
        reviewer,
        decider,
        &failing_check,
        &store,
    )
    .log
}

/// LOG holds LINE, whole.
fn assert_line(log: &str, line: &str) {
    assert!(
        log.contains(&format!("\n{line}\n")),
        "no line {line:?} in the run log:\n{log}"
    );
}

/// LOG holds LINE as the last of its section: the next section's heading
/// (or the log's end) follows after one blank line.
fn assert_section_end(log: &str, line: &str) {
    let ends = log.contains(&format!("\n{line}\n\n## ")) || log.ends_with(&format!("\n{line}\n"));
    assert!(ends, "no section ending {line:?} in the run log:\n{log}");
}

/// The run-log lines the goldens do not reach, each pinned exactly.
#[test]
fn the_run_log_lines_the_goldens_do_not_reach_are_as_recorded() {
    let nitpick = || StubDecider::new().on("nitpick", |_| true, StubDecider::p_yes(0.9));
    let same_nitpick = || {
        StubDecider::new()
            .on("same", |_| true, StubDecider::p_yes(0.9))
            .on("nitpick", |_| true, StubDecider::p_yes(0.9))
    };

    // nothing found, nothing to remember
    let f = Fixture::new();
    let log = log_of(&f, &StubReviewer::new(), &StubDecider::new());
    assert_section_end(&log, "- none found");
    assert_section_end(&log, "- nothing resolved or gone");

    // a failed call: not whole, nothing reviewed
    let f = Fixture::new();
    let failing = StubReviewer::new().reply(&["orders.py"], "I could not finish.");
    let log = log_of(&f, &failing, &StubDecider::new());
    assert_section_end(
        &log,
        "- the pass was not whole (a cap or a failure): nothing is resolved for not being found again",
    );
    assert_section_end(&log, "- no unit newly reviewed");

    // the first finding has no candidate; raised again, a withheld one is
    // not asked again on the same unit, rule and sides
    let f = Fixture::new();
    let log = log_of(&f, &bug_reviewer("cancel() returns None."), &nitpick());
    assert_line(
        &log,
        "- same-problem question: no stored finding shares its file, rule or cited code",
    );
    let log = log_of(&f, &bug_reviewer("cancel() returns None."), &same_nitpick());
    assert_line(
        &log,
        "- withheld, asked on the same unit, rule and sides: not asked again",
    );

    // two stored findings the same problem: the second answer is beaten
    let f = Fixture::new();
    let two = serde_json::json!({
        "findings": [
            {"issue_type": "bug", "file": "orders.py", "lines": [2, 2],
             "statement": "first", "rule": "", "evidence": [{"file": "orders.py", "lines": [2, 2]}]},
            {"issue_type": "bug", "file": "orders.py", "lines": [1, 1],
             "statement": "second", "rule": "", "evidence": [{"file": "orders.py", "lines": [1, 1]}]},
        ],
        "open": [],
    })
    .to_string();
    log_of(
        &f,
        &StubReviewer::new().reply(&["orders.py"], &two),
        &StubDecider::new(),
    );
    let [a, b] = [&f.store().findings[0].id, &f.store().findings[1].id].map(|i| i.0.clone());
    let log = log_of(&f, &bug_reviewer("third"), &same_nitpick());
    assert_line(
        &log,
        &format!(
            "- same problem as {a} (unverified, bug at orders.py:2; rule: none; first)? P(yes) 0.90 → the same: the first at or past the threshold wins"
        ),
    );
    assert_line(
        &log,
        &format!(
            "- same problem as {b} (unverified, bug at orders.py:1; rule: none; second)? P(yes) 0.90 → the same, but an earlier one won"
        ),
    );

    // the same as one dismissed in another unit: a match starts afresh
    let f = Fixture::new();
    log_of(
        &f,
        &bug_reviewer("cancel() returns None."),
        &StubDecider::new(),
    );
    let id = only(&f.store()).id.0.clone();
    support::update_store(&f.project, |s| {
        s.findings[0].place.unit = Some("place".into());
        s.dismiss(&id, Dismissal::Here, t0())
    })
    .unwrap()
    .unwrap();
    let log = log_of(
        &f,
        &bug_reviewer("cancel() gives back None."),
        &same_nitpick(),
    );
    assert_line(
        &log,
        &format!("- result: new ({id} is dismissed-here: a match starts afresh)"),
    );

    // the unit changes while the pass runs (and so before its checks)
    let f = Fixture::new();
    let root = f.root().to_path_buf();
    let moving = bug_reviewer("cancel() returns None.").on_review(move |_| {
        let p = root.join("orders.py");
        let text = std::fs::read_to_string(&p).unwrap();
        std::fs::write(p, text.replace("now=True", "now=False")).unwrap();
    });
    let log = log_of(&f, &moving, &StubDecider::new());
    assert_line(
        &log,
        "- `orders.py` `cancel` changed while the pass ran: its new findings are dropped, it is not reviewed",
    );
    assert_line(
        &log,
        "- `orders.py` `cancel` changed while the checks ran: not reviewed",
    );

    // three open in the file and one waiting from before: nobody fits
    let f = Fixture::new();
    let four = serde_json::json!({
        "findings": (1..=4).map(|n| serde_json::json!(
            {"issue_type": "bug", "file": "orders.py", "lines": [2, 2],
             "statement": format!("finding {n}"), "rule": "",
             "evidence": [{"file": "orders.py", "lines": [2, 2]}]}
        )).collect::<Vec<_>>(),
        "open": [],
    })
    .to_string();
    log_of(
        &f,
        &StubReviewer::new().reply(&["orders.py"], &four),
        &StubDecider::new(),
    );
    support::update_store(&f.project, |s| {
        for g in s.findings.iter_mut().take(3) {
            g.set_status(Status::Open, t0());
        }
    })
    .unwrap();
    let log = log_of(&f, &StubReviewer::new(), &StubDecider::new());
    assert_section_end(&log, "- nobody fits: the rounds end");
}

// ---- what an editor shows in a file ----

/// The fingerprint of `cancel` in the fixture's orders.py as it is now.
fn cancel_print_now(f: &Fixture) -> String {
    let units = f.view.units(Path::new("orders.py")).unwrap();
    let cancel = units.iter().find(|u| u.name == "cancel").unwrap();
    cancel.fingerprint.as_str().to_string()
}

#[test]
fn shown_in_file_places_what_scry_would_print() {
    use scry::usecases::show_findings::shown_in_file;
    let f = Fixture::new();
    let print = cancel_print_now(&f);
    let confidences = [0.5, 0.9, 0.7, 0.8];
    let findings = confidences
        .iter()
        .enumerate()
        .map(|(i, confidence)| {
            let mut finding = support::open_finding(
                &format!("f{i}"),
                "bug",
                "orders.py",
                Some(("cancel", 1, &print)),
                2,
            );
            finding.scores.confidence = Some(*confidence);
            finding
        })
        .collect();
    support::put_findings(&f.project, findings);
    let ports = support::Ports::of(&f.project);
    let shown = shown_in_file(
        &ports.store,
        &ports.settings,
        &f.view,
        Path::new("orders.py"),
    )
    .unwrap();
    let ids: Vec<&str> = shown.iter().map(|s| s.finding.id.0.as_str()).collect();
    assert_eq!(ids, vec!["f1", "f3", "f2"]);
    assert!(shown.iter().all(|s| s.placed
        == scry::domain::placing::Placed {
            line: 2,
            stale: false
        }));
}

#[test]
fn shown_in_file_is_empty_when_scry_is_off() {
    use scry::usecases::show_findings::shown_in_file;
    let f = Fixture::new();
    let print = cancel_print_now(&f);
    support::put_findings(
        &f.project,
        vec![support::open_finding(
            "f0",
            "bug",
            "orders.py",
            Some(("cancel", 1, &print)),
            2,
        )],
    );
    write(
        f.root(),
        ".sorcerer/config.toml",
        "[scry]\nenabled = false\n",
    );
    let (project, view) = support::project_with_vendored_grammars(f.root());
    let ports = support::Ports::of(&project);
    let shown =
        shown_in_file(&ports.store, &ports.settings, &view, Path::new("orders.py")).unwrap();
    assert!(shown.is_empty());
}

// ---- what an editor's pass needs from the engine ----

/// A review reply holding one finding of KIND on FILE's LINE, evidence on that line.
fn reply_in(file: &str, kind: &str, line: u32, statement: &str, rule: &str) -> String {
    serde_json::json!({
        "findings": [{
            "issue_type": kind,
            "file": file,
            "lines": [line, line],
            "statement": statement,
            "rule": rule,
            "evidence": [{"file": file, "lines": [line, line]}],
        }],
        "open": [],
    })
    .to_string()
}

/// How an editor runs a pass: on the files it names, within its caps, its review calls
/// stopped by their own cancel, its progress told as it goes.
struct EditorPass<'a> {
    files: &'a [&'a str],
    caps: Caps,
    review_cancel: &'a Cancel,
    on_progress: &'a (dyn Fn(&Progress) + Sync),
}

impl EditorPass<'_> {
    fn run(&self, f: &Fixture, reviewer: &StubReviewer, decider: &StubDecider) -> ReviewReport {
        let clock = FixedClock::at(t0());
        let ports = support::Ports::of(&f.project);
        let rest = Cancel::default();
        let deps = ReviewDeps {
            view: &f.view,
            reviewer,
            decider,
            checker: &failing_check,
            store: &ports.store,
            ledger: &ports.ledger,
            settings: &ports.settings,
            clock: &clock,
            log: &Quiet,
            progress: self.on_progress,
            cancel: &rest,
            review_cancel: self.review_cancel,
        };
        let req = ReviewRequest {
            caps: Some(self.caps.clone()),
            ..ReviewRequest::of(ScopeRequest::Files(
                self.files.iter().map(PathBuf::from).collect(),
            ))
        };
        ran(review_change(&f.project, &req, &deps).unwrap())
    }
}

fn no_caps() -> Caps {
    Caps {
        pass: None,
        day: f64::INFINITY,
        budget: None,
    }
}

/// A plain editor pass on orders.py, no caps, nobody cancelling, progress ignored.
fn editor_pass(f: &Fixture, reviewer: &StubReviewer) -> ReviewReport {
    EditorPass {
        files: &["orders.py"],
        caps: no_caps(),
        review_cancel: &Cancel::default(),
        on_progress: &|_| {},
    }
    .run(f, reviewer, &StubDecider::new())
}

fn convention_reviewer() -> StubReviewer {
    StubReviewer::new().reply(
        &["orders.py"],
        &reply(
            "convention",
            2,
            "cancel() can raise out of a command.",
            "result",
        ),
    )
}

#[test]
fn sending_names_the_units_of_each_call_before_it_goes() {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    let f = Fixture::new();
    let told = Arc::new(Mutex::new(Vec::<Progress>::new()));
    let seen_before_the_call = Arc::new(AtomicBool::new(false));
    let expected = scry::usecases::review_change::SentUnit {
        file: "orders.py".into(),
        unit: "cancel".into(),
        fingerprint: sorcerer_core::domain::fingerprint::Fingerprint::parse(&cancel_print(&f))
            .unwrap(),
    };
    let reviewer = {
        let told = Arc::clone(&told);
        let seen = Arc::clone(&seen_before_the_call);
        let expected = expected.clone();
        StubReviewer::new().on_review(move |_| {
            let sending = Progress::Sending(vec![expected.clone()]);
            seen.store(told.lock().unwrap().contains(&sending), Ordering::SeqCst);
        })
    };
    let record = |p: &Progress| told.lock().unwrap().push(p.clone());
    EditorPass {
        files: &["orders.py"],
        caps: no_caps(),
        review_cancel: &Cancel::default(),
        on_progress: &record,
    }
    .run(&f, &reviewer, &StubDecider::new());
    assert!(seen_before_the_call.load(Ordering::SeqCst));
    let sendings = told
        .lock()
        .unwrap()
        .iter()
        .filter(|p| matches!(p, Progress::Sending(_)))
        .count();
    assert_eq!(sendings, 1);
}

#[test]
fn nothing_sent_means_no_sending() {
    use std::sync::Mutex;
    let f = Fixture::new();
    let reviewer = StubReviewer::new();
    editor_pass(&f, &reviewer);
    let told = Mutex::new(Vec::<Progress>::new());
    let record = |p: &Progress| told.lock().unwrap().push(p.clone());
    EditorPass {
        files: &["orders.py"],
        caps: no_caps(),
        review_cancel: &Cancel::default(),
        on_progress: &record,
    }
    .run(&f, &reviewer, &StubDecider::new());
    assert_eq!(reviewer.asks().len(), 1);
    assert!(
        !told
            .lock()
            .unwrap()
            .iter()
            .any(|p| matches!(p, Progress::Sending(_))),
        "{told:?}"
    );
}

#[test]
fn review_cancel_stops_review_calls_and_nothing_else() {
    let f = Fixture::new();
    // a second file long enough that the two units go in two calls
    write(f.root(), "more.py", &long_unit("more", 700));
    let (project, view) = support::project_with_vendored_grammars(f.root());
    let f = Fixture { project, view, ..f };
    let review_cancel = Cancel::default();
    let reviewer = {
        let review_cancel = review_cancel.clone();
        StubReviewer::new()
            .reply(
                &["orders.py"],
                &reply(
                    "convention",
                    2,
                    "cancel() can raise out of a command.",
                    "result",
                ),
            )
            .reply(
                &["more.py"],
                &reply_in(
                    "more.py",
                    "convention",
                    2,
                    "more() can raise out of a command.",
                    "result",
                ),
            )
            .on_review(move |_| review_cancel.cancel())
    };
    let decider = StubDecider::new();
    let r = EditorPass {
        files: &["orders.py", "more.py"],
        caps: no_caps(),
        review_cancel: &review_cancel,
        on_progress: &|_| {},
    }
    .run(&f, &reviewer, &decider);
    assert_eq!(reviewer.asks().len(), 1);
    assert_eq!(r.failures.len(), 1, "{:?}", r.failures);
    assert!(
        r.failures[0].starts_with("review cancelled for "),
        "{:?}",
        r.failures
    );
    let s = f.store();
    assert_eq!(only(&s).status, Status::Open);
    assert!(!decider.asked().is_empty());
}

#[test]
fn a_unit_put_back_resolves_its_finding_without_a_call() {
    let f = Fixture::new();
    let reviewer = convention_reviewer();
    editor_pass(&f, &reviewer);
    assert_eq!(only(&f.store()).status, Status::Open);
    f.edit("order.cancel(now=True)", "order.cancel()");
    let r = editor_pass(&f, &reviewer);
    assert_eq!(only(&f.store()).status, Status::Resolved);
    assert_eq!(reviewer.asks().len(), 1);
    assert_eq!(r.touched.len(), 1);
}

#[test]
fn a_committed_change_keeps_its_finding_open() {
    let f = Fixture::new();
    let reviewer = convention_reviewer();
    editor_pass(&f, &reviewer);
    commit(f.root(), "the change, bug and all");
    editor_pass(&f, &reviewer);
    assert_eq!(only(&f.store()).status, Status::Open);
    assert_eq!(reviewer.asks().len(), 1);
}

#[test]
fn day_spent_is_reported_when_the_days_cap_holds_a_call_back() {
    let f = Fixture::new();
    let r = EditorPass {
        files: &["orders.py"],
        caps: Caps {
            pass: None,
            day: 0.0,
            budget: None,
        },
        review_cancel: &Cancel::default(),
        on_progress: &|_| {},
    }
    .run(&f, &StubReviewer::new(), &StubDecider::new());
    assert!(r.capped > 0);
    assert!(r.day_spent);
}

#[test]
fn day_spent_is_not_reported_when_only_the_pass_cap_holds_a_call_back() {
    let f = Fixture::new();
    let r = EditorPass {
        files: &["orders.py"],
        caps: Caps {
            pass: Some(0.0),
            day: f64::INFINITY,
            budget: None,
        },
        review_cancel: &Cancel::default(),
        on_progress: &|_| {},
    }
    .run(&f, &StubReviewer::new(), &StubDecider::new());
    assert!(r.capped > 0);
    assert!(!r.day_spent);
}
