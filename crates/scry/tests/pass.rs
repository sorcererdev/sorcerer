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
