#![allow(clippy::too_many_lines)]
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use scry::domain::budget::Caps;
use scry::domain::finding::{Finding, Status};
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
use sorcerer_models::domain::claim::{Checked, Claim};
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
        // a claim check that fails: a waiting finding stays waiting
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
    assert_eq!(g.status, Status::Waiting);
    assert_eq!(g.history, [(t0(), Status::Waiting)]);
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
    assert_eq!(g.status, Status::Waiting);
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
    support::update_store(&f.project, |s| s.dismiss(&id.0, true, t0()))
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
    assert_eq!(only(&s).status, Status::Waiting);
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
    // a waiting finding on `place`, which this pass does not send
    support::update_store(&f.project, |s| {
        let mut other = s.findings[0].clone();
        other.id = scry::domain::finding::FindingId("abcdef".into());
        other.place.unit = Some("place".into());
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
    assert_eq!(other.status, Status::Waiting);
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
    assert_eq!(only(&f.store()).status, Status::Waiting);

    // cancel changes again and big too; big's call fails: nothing resolves
    f.edit("cancel(now=False)", "cancel(now=True)");
    write(f.root(), "big.py", &big.replace("x = 1", "x = 2"));
    let failing = StubReviewer::new().reply(&["big.py"], "I could not finish.");
    let r = ran(f.run(&failing, &StubDecider::new(), false));
    assert_eq!(failing.asks().len(), 2);
    assert!(!r.whole);
    assert_eq!(r.failures.len(), 1);
    assert_eq!(only(&f.store()).status, Status::Waiting);

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
        support::update_store(&project, |s| s.dismiss(&dismissed.0, false, t0()))
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
    use sorcerer_models::domain::claim::{Cited, RawCitation, Verdict};
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
                    confirmed: true,
                    unit_missed: false,
                },
                Cited {
                    raw: RawCitation {
                        path: "orders.py".into(),
                        unit: Some("Order.refund".into()),
                        line: None,
                    },
                    confirmed: false,
                    unit_missed: false,
                },
            ],
            escalated: true,
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
    has("nitpick? P(yes) 0.20 → confidence 0.80, threshold 0.70 → waiting");
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
    // the first pass leaves a waiting finding on alpha and every unit reviewed
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
    assert_eq!(pass.progress.first(), Some(&Progress::Reviewed(1)));
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
