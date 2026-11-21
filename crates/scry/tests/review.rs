#![allow(clippy::too_many_lines)]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use scry::adapters::research_reviewer::ResearchReviewer;
use scry::domain::finding::{Finding, FindingId, Place, Scores, Status};
use scry::domain::review_prompt::*;
use scry::domain::steering::{Include, Rule, Steering};
use scry::ports::reviewer::Reviewer;
use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::unit::Span;
use sorcerer_models::domain::research::{Research, Researched, Tool};
use sorcerer_models::ports::researcher::Researcher;

fn sp(a: u32, z: u32) -> Span {
    Span { start: a, end: z }
}

/// Context, one include and two rules, the first of two lines.
fn fixture_steering() -> Steering {
    Steering {
        context: "Bistro is ports and adapters.".into(),
        includes: vec![Include {
            path: "docs/conventions.md".into(),
            text: "A command never panics.\n".into(),
        }],
        rules: vec![
            Rule {
                id: "ports".into(),
                text: "Commands go through ports (must)\nA command never touches the database."
                    .into(),
            },
            Rule {
                id: "clock".into(),
                text: "Time comes from the clock port.".into(),
            },
        ],
    }
}

fn ask0() -> ReviewAsk {
    let now = Utc::now();
    ReviewAsk {
        units: vec![
            ReviewUnit {
                file: "src/a.py".into(),
                name: "A.f".into(),
                lines: sp(3, 5),
                header: Some("class A:".into()),
                text: "def f(self, x):\n    return x\n    # end".into(),
                in_progress: false,
            },
            ReviewUnit {
                file: "src/b.py".into(),
                name: "g".into(),
                lines: sp(1, 2),
                header: None,
                text: "def g():\n    pass".into(),
                in_progress: false,
            },
        ],
        diff: "--- a/src/a.py\n+++ b/src/a.py\n@@ -3 +3 @@\n-x\n+y\n".into(),
        steering: fixture_steering().steering_text(),
        reported: vec!["rule r at src/a.py:4".into()],
        open: vec![Finding {
            id: FindingId("abc123".into()),
            issue_type: "bug".into(),
            rule: None,
            statement: "It returns x unchanged.".into(),
            place: Place {
                file: "src/a.py".into(),
                unit: Some("A.f".into()),
                lines: sp(3, 3),
                unit_start: None,
                fingerprint: None,
            },
            evidence: vec![],
            scores: Scores::default(),
            verdict: None,
            status: Status::Open,
            seen: 0,
            first: now,
            last: now,
            history: vec![],
            model: "sonnet".into(),
            cost_usd: 0.0,
        }],
    }
}

fn unit(n: u32) -> ReviewUnit {
    ReviewUnit {
        file: "a.py".into(),
        name: format!("u{n}"),
        lines: sp(1, n),
        header: None,
        text: String::new(),
        in_progress: false,
    }
}

#[test]
fn the_prompt_matches_the_golden() {
    let golden = std::fs::read_to_string("tests/golden/review-prompt.txt").unwrap();
    assert_eq!(review_prompt(&ask0()), golden);
}

#[test]
fn an_in_progress_unit_is_marked() {
    let mut a = ask0();
    a.units[0].in_progress = true;
    let p = review_prompt(&a);
    assert!(p.contains("--- src/a.py:3-5 A.f (in `class A:`) (in progress)"));
    assert!(p.contains("Units marked (in progress) are still being written"));
    assert!(!review_prompt(&ask0()).contains("in progress"));
}

fn one(r: &str) -> ReviewReply {
    parse_review(r).unwrap()
}

const F: &str =
    r#"{"issue_type": "bug", "file": "src/a.py", "lines": [3, 4], "statement": "Wrong."}"#;

fn raw(rule: Option<&str>, evidence: Vec<(PathBuf, Span)>) -> RawFinding {
    RawFinding {
        issue_type: "bug".into(),
        file: "src/a.py".into(),
        lines: sp(3, 4),
        statement: "Wrong.".into(),
        rule: rule.map(Into::into),
        evidence,
    }
}

#[test]
fn parses_the_last_fenced_json() {
    let r =
        format!("```json\n{{\"findings\": []}}\n```\nthen\n```json\n{{\"findings\": [{F}]}}\n```");
    assert_eq!(one(&r).findings, vec![raw(None, vec![])]);
}

#[test]
fn parses_a_bare_object_after_prose() {
    let r = r#"I looked.

{"findings": [{"issue_type": "bug", "file": "src/a.py", "lines": [3, 4], "statement": "Wrong.", "rule": " A ", "evidence": [{"file": "src/b.py", "lines": [1, 2]}]}], "open": [{"id": "abc123", "holds": "no longer holds", "why": "fixed"}]}"#;
    let got = one(r);
    assert_eq!(
        got.findings,
        vec![raw(Some("A"), vec![("src/b.py".into(), sp(1, 2))])]
    );
    assert_eq!(
        got.open,
        vec![OpenAnswer {
            id: "abc123".into(),
            holds: Holds::NoLonger,
            why: "fixed".into()
        }]
    );
}

#[test]
fn a_top_level_array_is_the_findings() {
    let got = one(&format!("here: [{F}]"));
    assert_eq!(got.findings, vec![raw(None, vec![])]);
    assert!(got.open.is_empty());
}

#[test]
fn a_one_element_lines_is_one_line() {
    let got = one(
        r#"{"findings": [{"issue_type": "bug", "file": "src/a.py", "lines": [7], "statement": "s"}]}"#,
    );
    assert_eq!(got.findings[0].lines, sp(7, 7));
}

#[test]
fn line_alone_is_accepted() {
    let got = one(
        r#"{"findings": [{"issue_type": "bug", "file": "src/a.py", "line": 9, "statement": "s"}]}"#,
    );
    assert_eq!(got.findings[0].lines, sp(9, 9));
}

#[test]
fn an_unknown_issue_type_or_blank_statement_drops_the_finding() {
    let got = one(
        r#"{"findings": [{"issue_type": "style", "file": "a", "lines": [1, 1], "statement": "s"},
        {"issue_type": "bug", "file": "a", "lines": [1, 1], "statement": "  "},
        {"issue_type": "bug", "file": "a", "lines": [1, 1]}]}"#,
    );
    assert!(got.findings.is_empty());
}

#[test]
fn a_bad_evidence_entry_is_dropped_and_the_finding_kept() {
    let got = one(
        r#"{"findings": [{"issue_type": "bug", "file": "a", "lines": [1, 1], "statement": "s",
        "evidence": [{"file": "x"}, {"file": "y", "lines": [2, 3]}]}]}"#,
    );
    assert_eq!(got.findings[0].evidence, vec![("y".into(), sp(2, 3))]);
}

#[test]
fn open_answers_need_exact_holds_words() {
    let got = one(
        r#"{"findings": [], "open": [{"id": 7, "holds": "no longer holds", "why": "x"},
        {"id": "b", "holds": "maybe"}, {"id": "c", "holds": "still holds"}]}"#,
    );
    assert_eq!(
        got.open,
        vec![
            OpenAnswer {
                id: "7".into(),
                holds: Holds::NoLonger,
                why: "x".into()
            },
            OpenAnswer {
                id: "c".into(),
                holds: Holds::Still,
                why: String::new()
            },
        ]
    );
}

#[test]
fn no_json_is_an_error() {
    assert!(
        parse_review("I found nothing")
            .unwrap_err()
            .contains("no JSON")
    );
}

#[test]
fn units_batch_up_to_600_lines() {
    let batches = batch_units(vec![unit(300), unit(250), unit(100)]);
    let sizes: Vec<Vec<u32>> = batches
        .iter()
        .map(|p| p.iter().map(|u| u.lines.end).collect())
        .collect();
    assert_eq!(sizes, vec![vec![300, 250], vec![100]]);
}

#[test]
fn a_long_unit_goes_alone() {
    let batches = batch_units(vec![unit(700), unit(10)]);
    assert_eq!(batches.iter().map(Vec::len).collect::<Vec<_>>(), vec![1, 1]);
}

struct Stub(Mutex<Option<Research>>);

impl Researcher for Stub {
    fn research(&self, r: Research, _: &Cancel) -> sorcerer_models::Result<Researched> {
        let errand = Errand {
            at: Utc::now(),
            product: r.product.clone(),
            verb: r.verb.clone(),
            model: r.model.clone(),
            provider: "stub".into(),
            cost_usd: Some(0.01),
            input_tokens: 0,
            output_tokens: 0,
            seconds: 0.0,
            ok: true,
            note: None,
        };
        *self.0.lock().unwrap() = Some(r);
        Ok(Researched {
            answer: "{}".into(),
            errand,
            reads: vec![],
        })
    }
}

#[test]
fn research_reviewer_asks_for_the_review_errand() {
    let stub = Arc::new(Stub(Mutex::new(None)));
    let ask = ask0();
    let found = ResearchReviewer {
        researcher: stub.clone(),
    }
    .review(&ask, "sonnet", 0.5, &Cancel::default())
    .unwrap();
    assert_eq!(found.reply, "{}");
    let r = stub.0.lock().unwrap().take().unwrap();
    assert_eq!((r.product.as_str(), r.verb.as_str()), ("scry", "review"));
    assert_eq!(r.tools, vec![Tool::Read, Tool::Grep, Tool::Glob]);
    assert_eq!(r.timeout.as_secs(), 420);
    assert_eq!(r.prompt, review_prompt(&ask));
    assert_eq!((r.model.as_str(), r.cap_usd), ("sonnet", 0.5));
}

#[test]
fn braces_in_the_prose_before_the_json_are_skipped() {
    let reply = "The test's expected string matches the \"Booked: {} for {}\" format.\n\n{\"findings\": [], \"open\": []}";
    assert!(parse_review(reply).unwrap().findings.is_empty());
    assert_eq!(
        json_in(reply),
        Some(serde_json::json!({"findings": [], "open": []}))
    );
}

#[test]
fn brackets_in_the_prose_do_not_turn_into_a_trailing_empty_list() {
    let reply = "It locks in the panic with `#[should_panic]`.\n\n{\"findings\": [{\"issue_type\": \"bug\", \"file\": \"a.rs\", \"lines\": [1, 2], \"statement\": \"s\", \"rule\": \"\", \"evidence\": []}], \"open\": []}";
    assert_eq!(parse_review(reply).unwrap().findings.len(), 1);
}
