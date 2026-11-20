#![allow(clippy::too_many_lines)]
mod support;

use std::path::{Path, PathBuf};

use chrono::Utc;
use scry::domain::finding::{Evidence, Finding, FindingId, Place, Scores, Status};
use scry::domain::findings::Findings;
use scry::domain::judging::{
    Match, NITPICK_IS, SAME_THRESHOLD, WORTH_SAYING_IS, asked_on, candidates, cited_code,
    confidence_of, finding_text, match_stored, nitpick, place, same_problem,
};
use scry::domain::review_prompt::RawFinding;
use scry::domain::scope::ScopedUnit;
use scry::domain::steering::{Rule, Steering};
use scry::usecases::review_change::verify::citations;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::fingerprint::{Fingerprint, fingerprint};
use sorcerer_core::domain::unit::{Span, Unit, UnitKind};
use sorcerer_models::domain::decide::{Answer, Confidence, Shape, UnsureAnswer, Weighed};
use support::{git_project, project_with_vendored_grammars, write};

fn span(start: u32, end: u32) -> Span {
    Span { start, end }
}

fn unit(name: &str, kind: UnitKind, start: u32, end: u32) -> Unit {
    Unit {
        name: name.into(),
        kind,
        span: span(start, end),
        body: None,
        fingerprint: Fingerprint::parse("abcdef0").unwrap(),
    }
}

fn scoped(u: &Unit) -> ScopedUnit {
    ScopedUnit {
        file: "a.py".into(),
        unit: u.clone(),
        header: None,
    }
}

fn raw(lines: Span) -> RawFinding {
    RawFinding {
        issue_type: "bug".into(),
        file: "a.py".into(),
        lines,
        statement: "cancel() returns None".into(),
        rule: None,
        evidence: vec![],
    }
}

fn stored(status: Status, file: &str, unit: Option<&str>) -> Finding {
    let now = Utc::now();
    Finding {
        id: FindingId("abc123".into()),
        issue_type: "bug".into(),
        rule: None,
        statement: "s".into(),
        place: Place {
            file: file.into(),
            unit: unit.map(String::from),
            lines: span(1, 2),
            unit_start: None,
            fingerprint: None,
        },
        evidence: vec![],
        scores: Scores::default(),
        verdict: None,
        status,
        seen: 1,
        first: now,
        last: now,
        history: vec![],
        model: "sonnet".into(),
        cost_usd: 0.0,
    }
}

/// A placement as (place, status), as the store will record it.
fn as_pair(p: scry::domain::judging::Placement) -> (scry::domain::finding::Place, Status) {
    match p {
        scry::domain::judging::Placement::InReviewedUnit(pl) => (pl, Status::Open),
        scry::domain::judging::Placement::Elsewhere(pl) => (pl, Status::Elsewhere),
    }
}

#[test]
fn place_prefers_the_unit_holding_the_first_line_then_the_narrowest() {
    let class = unit("Order", UnitKind::Class, 1, 30);
    let method = unit("Order.cancel", UnitKind::Method, 10, 20);
    let units = [class.clone(), method.clone()];
    let (pl, st) = as_pair(place(
        &raw(span(12, 14)),
        &[scoped(&class), scoped(&method)],
        &units,
    ));
    assert_eq!(st, Status::Open);
    assert_eq!(pl.unit.as_deref(), Some("Order.cancel"));
    assert_eq!(pl.unit_start, Some(10));
    assert_eq!(pl.lines, span(12, 14));
    // a range starting before a scoped unit and running into it is in it
    let (pl, st) = as_pair(place(&raw(span(8, 12)), &[scoped(&method)], &units));
    assert_eq!(
        (pl.unit.as_deref(), st),
        (Some("Order.cancel"), Status::Open)
    );
}

#[test]
fn place_outside_scoped_units_is_elsewhere() {
    let class = unit("Order", UnitKind::Class, 1, 30);
    let method = unit("Order.cancel", UnitKind::Method, 10, 20);
    let (pl, st) = as_pair(place(
        &raw(span(25, 26)),
        &[scoped(&method)],
        &[class.clone(), method.clone()],
    ));
    assert_eq!(st, Status::Elsewhere);
    assert_eq!(pl.unit.as_deref(), Some("Order"));
    // another file's scoped unit does not count
    let mut other = scoped(&method);
    other.file = "b.py".into();
    let (pl, st) = as_pair(place(&raw(span(12, 12)), &[other], &[]));
    assert_eq!((pl.unit, st), (None, Status::Elsewhere));
}

#[test]
fn candidates_share_the_file_the_rule_or_a_cited_range_and_skip_resolved_and_gone() {
    let mut by_rule = stored(Status::Open, "z.py", None);
    by_rule.rule = Some("R".into());
    let mut by_evidence = stored(Status::Unverified, "y.py", None);
    by_evidence.evidence = vec![Evidence {
        file: "c.py".into(),
        lines: span(5, 9),
        print: None,
    }];
    let store = Findings {
        findings: vec![
            stored(Status::Open, "a.py", None),
            stored(Status::Resolved, "a.py", None),
            stored(Status::Gone, "a.py", None),
            stored(Status::Open, "x.py", None),
            by_rule,
            by_evidence,
        ],
        ..Default::default()
    };
    let mut r = raw(span(1, 1));
    r.rule = Some("R".into());
    r.evidence = vec![(PathBuf::from("c.py"), span(9, 12))];
    let files: Vec<&str> = candidates(&store, &r)
        .iter()
        .map(|f| f.place.file.to_str().unwrap())
        .collect();
    assert_eq!(files, ["a.py", "z.py", "y.py"]);
}

#[test]
fn same_problem_question_has_inspects_wording() {
    let q = same_problem("A", "B", "ca", "cb");
    assert_eq!(q.kind, "same");
    assert_eq!(
        q.instructions,
        "Do these two code review findings describe the same problem?"
    );
    assert_eq!(
        q.shape,
        Shape::Noul {
            yes: "one fix would fix both: the same defect or rule break at the same code, however it is worded".into(),
            no: "they need separate fixes: different defects, or the same kind of defect at different code".into(),
        }
    );
    let names: Vec<&str> = q.context.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["finding A", "finding B", "code at A", "code at B"]);
    let values: Vec<&str> = q.context.iter().map(|(_, v)| v.as_str()).collect();
    assert_eq!(values, ["A", "B", "ca", "cb"]);
    assert_eq!(q.unsure, UnsureAnswer::NotAllowed);
    assert_eq!(SAME_THRESHOLD, 0.5);
}

#[test]
fn finding_text_reads() {
    assert_eq!(
        finding_text("bug", Path::new("a.py"), 41, None, "cancel() returns None"),
        "bug at a.py:41; rule: none; cancel() returns None"
    );
    assert_eq!(
        finding_text("convention", Path::new("a.py"), 3, Some("R"), "s"),
        "convention at a.py:3; rule: R; s"
    );
}

#[test]
fn match_table() {
    let m = |s: Status| match_stored(&stored(s, "a.py", Some("f")), Some("f"));
    assert_eq!(m(Status::Open), Match::MergeInto);
    assert_eq!(m(Status::Unverified), Match::MergeInto);
    assert_eq!(m(Status::Withheld), Match::MergeInto);
    assert_eq!(m(Status::Refuted), Match::MergeInto);
    assert_eq!(m(Status::Unsure), Match::MergeInto);
    assert_eq!(m(Status::Elsewhere), Match::MergeInto);
    assert_eq!(m(Status::DismissedHere), Match::Drop);
    let here = stored(Status::DismissedHere, "a.py", Some("f"));
    assert_eq!(match_stored(&here, Some("g")), Match::AddAsNew);
    assert_eq!(match_stored(&here, None), Match::AddAsNew);
    assert_eq!(m(Status::DismissedEverywhere), Match::Drop);
    assert_eq!(m(Status::Resolved), Match::AddAsNew);
    assert_eq!(m(Status::Gone), Match::AddAsNew);
}

fn rules() -> Steering {
    Steering {
        rules: vec![
            Rule {
                id: "result".into(),
                text: "Commands return a Result (must)\nA command never panics.\n".into(),
            },
            Rule {
                id: "plain-names".into(),
                text: "Name things plainly\nNo abbreviations.".into(),
            },
        ],
        ..Steering::default()
    }
}

#[test]
fn nitpick_carries_the_cited_rules_text_or_all_rules() {
    let st = rules();
    let mut r = raw(span(12, 14));
    r.rule = Some("result".into());
    let q = nitpick(&st, &r, "code");
    assert_eq!(q.kind, "nitpick");
    assert_eq!(q.instructions, "Is this code review finding a nitpick?");
    assert_eq!(
        q.context,
        [
            ("issue type".to_string(), "bug".to_string()),
            ("finding".into(), "cancel() returns None".into()),
            ("place".into(), "a.py:12".into()),
            ("cited code".into(), "code".into()),
            (
                "project rule".into(),
                "Commands return a Result (must)\nA command never panics.".into()
            ),
        ]
    );
    assert_eq!(q.unsure, UnsureAnswer::NotAllowed);

    r.rule = None;
    let q = nitpick(&st, &r, "code");
    let last = q.context.last().unwrap();
    assert_eq!(last.0, "project rules");
    assert_eq!(last.1, st.rules_text());
    assert!(last.1.contains("- plain-names: Name things plainly"));

    let q = nitpick(&Steering::default(), &r, "code");
    assert_eq!(q.context.last().unwrap().1, "(none)");
}

#[test]
fn the_sides_are_the_constants() {
    let q = nitpick(&rules(), &raw(span(12, 14)), "code");
    assert_eq!(
        q.shape,
        Shape::Noul {
            yes: NITPICK_IS.into(),
            no: WORTH_SAYING_IS.into()
        }
    );
    assert_eq!(
        NITPICK_IS,
        "it is about taste or polish: naming, wording, formatting, style, a small optional refactor, or speculative future-proofing; nothing would go wrong if it were ignored"
    );
    assert_eq!(
        WORTH_SAYING_IS,
        "ignoring it leaves wrong behaviour, a failure, lost data, a security hole, or a real maintenance cost"
    );
}

#[test]
fn confidence_of_values() {
    let k = |a: Answer| {
        confidence_of(&Weighed {
            answer: a,
            distribution: vec![],
        })
    };
    assert!((k(Answer::Yes(Confidence::Known(0.9))) - 0.1).abs() < 1e-9);
    assert!((k(Answer::No(Confidence::Known(0.8))) - 0.8).abs() < 1e-9);
    assert_eq!(k(Answer::Yes(Confidence::Unknown)), 0.0);
    assert_eq!(k(Answer::No(Confidence::Unknown)), 1.0);
    assert_eq!(k(Answer::Unsure), 1.0);
}

#[test]
fn asked_on_changes_with_the_unit_and_the_rule_text() {
    let fingerprint = Fingerprint::parse("abcdef0").unwrap();
    let other = Fingerprint::parse("1234567").unwrap();
    let h = asked_on(Some(&fingerprint), "rule");
    assert_eq!(h.len(), 7);
    assert_eq!(h, asked_on(Some(&fingerprint), "rule"));
    assert_ne!(h, asked_on(Some(&other), "rule"));
    assert_ne!(h, asked_on(None, "rule"));
    assert_ne!(h, asked_on(Some(&fingerprint), "rule 2"));
}

#[test]
fn cited_code_is_eight_lines_either_side_numbered_to_four() {
    let text: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    let c = cited_code(&text, 12);
    let ls: Vec<&str> = c.lines().collect();
    assert_eq!(ls.len(), 17);
    assert_eq!(ls[0], "   4 line 4");
    assert_eq!(ls[16], "  20 line 20");
    // clamped at both ends
    let c = cited_code(&text, 3);
    assert!(c.starts_with("   1 line 1\n"));
    assert!(cited_code(&text, 28).ends_with("  30 line 30"));
}

fn fixture() -> tempfile::TempDir {
    let d = git_project();
    let body: String = (1..=59).map(|n| format!("x{n} = {n}\n")).collect();
    write(d.path(), "src/long.py", &body);
    write(
        d.path(),
        "src/a.py",
        "import os\n\ndef cancel(order):\n    order.state = 'x'\n    return None\n",
    );
    write(d.path(), ".env", "SECRET=1\n");
    write(d.path(), ".sorcerer/state/x.txt", "hidden\n");
    d
}

#[test]
fn citations_refuse_a_missing_file_a_fenced_file_and_a_line_past_the_end() {
    let d = fixture();
    let (p, view) = project_with_vendored_grammars(d.path());
    let fences = Fences::for_project(&p).unwrap();
    let bad = |path: &str, s: Span| {
        assert_eq!(
            citations(&view, &fences, &[(PathBuf::from(path), s)]),
            Err("cited code not found".to_string()),
            "{path}"
        );
    };
    bad("src/missing.py", span(1, 1));
    bad(".env", span(1, 1));
    bad(".sorcerer/state/x.txt", span(1, 1));
    bad("../outside.py", span(1, 1));
    bad(d.path().join("src/a.py").to_str().unwrap(), span(1, 1));
    bad("src/long.py", span(60, 61));
    bad("src/long.py", span(0, 1));
    // one bad citation among good ones fails them all
    assert!(
        citations(
            &view,
            &fences,
            &[
                (PathBuf::from("src/a.py"), span(3, 5)),
                (PathBuf::from("src/missing.py"), span(1, 1))
            ]
        )
        .is_err()
    );
}

#[test]
fn citations_print_the_enclosing_unit_else_the_whole_file() {
    let d = fixture();
    let (p, view) = project_with_vendored_grammars(d.path());
    let fences = Fences::for_project(&p).unwrap();
    let text = std::fs::read_to_string(d.path().join("src/a.py")).unwrap();
    let ev = citations(
        &view,
        &fences,
        &[
            (PathBuf::from("src/a.py"), span(4, 5)),
            (PathBuf::from("src/a.py"), span(1, 1)),
        ],
    )
    .unwrap();
    assert_eq!(ev[0].file, PathBuf::from("src/a.py"));
    assert_eq!(ev[0].lines, span(4, 5));
    assert_eq!(
        ev[0].print.as_deref(),
        Some(fingerprint(&text, span(3, 5)).as_str())
    );
    let whole = ev[1].print.clone().unwrap();
    assert_eq!(whole.len(), 7);
    assert_ne!(whole, ev[0].print.clone().unwrap());
}

#[test]
fn citations_ignore_the_end_line() {
    // the vm05 case: 50-60 cited in a 59-line file
    let d = fixture();
    let (p, view) = project_with_vendored_grammars(d.path());
    let fences = Fences::for_project(&p).unwrap();
    let ev = citations(
        &view,
        &fences,
        &[(PathBuf::from("src/long.py"), span(50, 60))],
    )
    .unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].lines, span(50, 60));
}

#[test]
fn no_evidence_is_its_own_reason() {
    let d = fixture();
    let (p, view) = project_with_vendored_grammars(d.path());
    let fences = Fences::for_project(&p).unwrap();
    assert_eq!(
        citations(&view, &fences, &[]),
        Err("no evidence cited".to_string())
    );
}
