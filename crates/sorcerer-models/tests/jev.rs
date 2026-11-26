#![allow(clippy::too_many_lines)]
mod support;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::config::DecideConfig;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_models::adapters::jev::{
    Jev, jev_answers, jev_body, question_key, requests_jev_takes,
};
use sorcerer_models::adapters::recorded_decider::RecordedDecider;
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Shape, UnsureAnswer};
use sorcerer_models::ports::decider::Decider;

fn noul(kind: &str, ctx: &[(&str, &str)]) -> Question {
    Question {
        kind: kind.into(),
        instructions: "Is this a nitpick?".into(),
        shape: Shape::Noul {
            yes: "taste".into(),
            no: "a real cost".into(),
        },
        context: ctx
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        unsure: UnsureAnswer::NotAllowed,
    }
}

fn choice(kind: &str) -> Question {
    Question {
        kind: kind.into(),
        instructions: "Which?".into(),
        shape: Shape::Choice(vec![
            ("a".into(), "first".into()),
            ("b".into(), "second".into()),
        ]),
        context: vec![],
        unsure: UnsureAnswer::Allowed,
    }
}

#[test]
fn the_body_merges_state_and_keys_questions_by_kind() {
    let b = jev_body(&[
        noul("n1", &[("finding", "f"), ("code", "c")]),
        noul("n2", &[("finding", "f")]),
        choice("c1"),
    ])
    .unwrap();
    assert_eq!(
        b,
        json!({
            "state": {"finding": "f", "code": "c"},
            "questions": {
                "n1": {"type": "noul", "instructions": "Is this a nitpick?", "criteria": {"true": "taste", "false": "a real cost"}},
                "n2": {"type": "noul", "instructions": "Is this a nitpick?", "criteria": {"true": "taste", "false": "a real cost"}},
                "c1": {"type": "choice", "instructions": "Which?", "criteria": {"a": "first", "b": "second", "unsure": "none of these fits"}}
            }
        })
    );
}

#[test]
fn a_conflicting_context_is_an_error() {
    assert!(jev_body(&[noul("n1", &[("x", "1")]), noul("n2", &[("x", "2")])]).is_err());
}

#[test]
fn two_questions_of_one_kind_is_an_error() {
    assert!(jev_body(&[noul("n", &[]), noul("n", &[])]).is_err());
}

#[test]
fn a_noul_above_half_is_yes_and_below_is_no_with_the_complement() {
    let qs = [noul("a", &[]), noul("b", &[])];
    let w = jev_answers(
        &qs,
        br#"{"answers": {"a": {"type": "noul", "noul": 0.8}, "b": {"type": "noul", "noul": 0.3}}}"#,
    );
    assert_eq!(w[0].answer, Answer::Yes(Confidence::Known(0.8)));
    match &w[1].answer {
        Answer::No(Confidence::Known(p)) => assert!((p - 0.7).abs() < 1e-12),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_missing_answer_makes_the_batch_unsure() {
    let qs = [noul("a", &[]), noul("b", &[])];
    let w = jev_answers(&qs, br#"{"answers": {"a": {"type": "noul", "noul": 0.8}}}"#);
    assert!(w.iter().all(|x| x.answer == Answer::Unsure));
    assert!(
        jev_answers(&qs, b"not json")
            .iter()
            .all(|x| x.answer == Answer::Unsure)
    );
}

#[test]
fn a_choice_reads_its_distribution_and_unsure_is_unsure() {
    let q = [choice("c")];
    let w = jev_answers(&q, br#"{"answers": {"c": {"type": "choice", "choice": "b", "confidence": 0.9, "probabilities": {"a": 0.1, "b": 0.9}}}}"#);
    assert_eq!(
        w[0].answer,
        Answer::Chosen("b".into(), Confidence::Known(0.9))
    );
    assert_eq!(
        w[0].distribution,
        vec![("b".to_string(), 0.9), ("a".to_string(), 0.1)]
    );
    let w = jev_answers(
        &q,
        br#"{"answers": {"c": {"type": "choice", "choice": "unsure"}}}"#,
    );
    assert_eq!(w[0].answer, Answer::Unsure);
    let w = jev_answers(
        &q,
        br#"{"answers": {"c": {"type": "choice", "choice": "z"}}}"#,
    );
    assert_eq!(w[0].answer, Answer::Unsure);
}

fn jev(url: &str, ledger: Option<Arc<dyn Ledger>>) -> Jev {
    Jev::from_env(
        &DecideConfig::default(),
        &MapEnv::default().var_set("JEV_API_KEY", "jk"),
        ledger,
    )
    .unwrap()
    .with_url(url)
    .with_backoff(Duration::ZERO)
    .for_product("scry")
}

#[test]
fn jev_posts_with_the_bearer_key_and_the_pinned_model_and_records_the_call() {
    let mut s = mockito::Server::new();
    let m = s
        .mock("POST", "/v1/systemone")
        .match_header("authorization", "Bearer jk")
        .match_body(mockito::Matcher::PartialJson(
            json!({"model": "jev-1.14.0", "questions": {"a": {"type": "noul"}}}),
        ))
        .with_body(
            r#"{"answers": {"a": {"type": "noul", "noul": 0.9}}, "usage": {"input_tokens": 120}}"#,
        )
        .create();
    let d = tempfile::TempDir::new().unwrap();
    let ledger = JsonlLedger::at(d.path().join("l.jsonl"));
    let w = jev(&s.url(), Some(Arc::new(ledger.clone())))
        .ask(&[noul("a", &[])], &Cancel::default())
        .unwrap();
    m.assert();
    assert_eq!(w[0].answer, Answer::Yes(Confidence::Known(0.9)));
    let rows = ledger.read().unwrap().errands;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].product.as_str(),
            rows[0].verb.as_str(),
            rows[0].provider.as_str(),
            rows[0].input_tokens
        ),
        ("scry", "decide", "jev", 120)
    );
}

#[test]
fn a_529_is_retried() {
    let mut s = mockito::Server::new();
    s.mock("POST", "/v1/systemone")
        .with_status(529)
        .expect(1)
        .create();
    s.mock("POST", "/v1/systemone")
        .with_body(r#"{"answers": {"a": {"type": "noul", "noul": 0.2}}}"#)
        .create();
    let w = jev(&s.url(), None)
        .ask(&[noul("a", &[])], &Cancel::default())
        .unwrap();
    assert!(matches!(w[0].answer, Answer::No(_)));
}

#[test]
fn a_failed_call_is_unsure_for_the_batch_and_recorded_not_ok() {
    let mut s = mockito::Server::new();
    s.mock("POST", "/v1/systemone").with_status(500).create();
    let d = tempfile::TempDir::new().unwrap();
    let ledger = JsonlLedger::at(d.path().join("l.jsonl"));
    let w = jev(&s.url(), Some(Arc::new(ledger.clone())))
        .ask(&[noul("a", &[]), noul("b", &[])], &Cancel::default())
        .unwrap();
    assert!(w.iter().all(|x| x.answer == Answer::Unsure));
    assert!(!ledger.read().unwrap().errands[0].ok);
}

#[test]
fn no_key_is_an_error() {
    let Err(e) = Jev::from_env(&DecideConfig::default(), &MapEnv::default(), None) else {
        panic!()
    };
    assert!(e.to_string().contains("JEV_API_KEY"), "{e}");
}

#[test]
fn the_question_key_is_stable_across_runs() {
    assert_eq!(
        question_key(&noul("a", &[("finding", "f")])),
        "c2ef36b5330f3c19"
    );
}

#[test]
fn recorded_decider_answers_by_key_and_misses_loudly() {
    let d = tempfile::TempDir::new().unwrap();
    let q = noul("a", &[("finding", "f")]);
    std::fs::write(
        d.path().join(format!("{}.json", question_key(&q))),
        r#"{"type": "noul", "noul": 0.75}"#,
    )
    .unwrap();
    let r = RecordedDecider::new(d.path().into());
    let w = r.ask(&[q], &Cancel::default()).unwrap();
    assert_eq!(w[0].answer, Answer::Yes(Confidence::Known(0.75)));
    assert!(r.ask(&[noul("b", &[])], &Cancel::default()).is_err());
}

#[test]
fn a_batch_jev_refuses_whole_is_split_into_requests_it_takes() {
    let same_about = |stored: &str| noul("same", &[("finding A", "new"), ("finding B", stored)]);
    // one request per kind; a context name with two texts needs a request of its own
    assert_eq!(
        requests_jev_takes(&[same_about("one"), same_about("two"), noul("nitpick", &[])]),
        vec![vec![0, 2], vec![1]]
    );
    assert_eq!(
        requests_jev_takes(&[
            noul("a", &[("code", "c")]),
            noul("b", &[("code", "other")]),
            noul("c", &[("code", "c")]),
        ]),
        vec![vec![0, 2], vec![1]]
    );
    assert!(requests_jev_takes(&[]).is_empty());
}

#[test]
fn two_questions_of_one_kind_are_asked_in_two_requests_and_answered_in_order() {
    let mut s = mockito::Server::new();
    let first = s
        .mock("POST", "/v1/systemone")
        .match_body(mockito::Matcher::PartialJson(
            json!({"state": {"finding B": "one"}}),
        ))
        .with_body(r#"{"answers": {"same": {"type": "noul", "noul": 0.9}}}"#)
        .expect(1)
        .create();
    let second = s
        .mock("POST", "/v1/systemone")
        .match_body(mockito::Matcher::PartialJson(
            json!({"state": {"finding B": "two"}}),
        ))
        .with_body(r#"{"answers": {"same": {"type": "noul", "noul": 0.2}}}"#)
        .expect(1)
        .create();
    let d = tempfile::TempDir::new().unwrap();
    let ledger = JsonlLedger::at(d.path().join("l.jsonl"));
    let w = jev(&s.url(), Some(Arc::new(ledger.clone())))
        .ask(
            &[
                noul("same", &[("finding A", "new"), ("finding B", "one")]),
                noul("same", &[("finding A", "new"), ("finding B", "two")]),
            ],
            &Cancel::default(),
        )
        .unwrap();
    first.assert();
    second.assert();
    assert_eq!(w[0].answer, Answer::Yes(Confidence::Known(0.9)));
    assert!(matches!(w[1].answer, Answer::No(_)));
    assert_eq!(ledger.read().unwrap().errands.len(), 2);
}
