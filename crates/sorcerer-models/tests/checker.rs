mod support;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sorcerer_core::fences::Fences;
use sorcerer_core::grammar::GrammarCache;
use sorcerer_core::language::GrammarSource;
use sorcerer_core::unit::Span;
use sorcerer_core::view::FilesView;
use sorcerer_core::{Cancel, Project};
use sorcerer_models::checker::prompts::claim_prompt;
use sorcerer_models::checker::{CheckerDeps, Citation, Claim, ClaimKind, Verdict, check};
use sorcerer_models::decide::{Answer, Confidence, Decider, Question, Weighed};
use sorcerer_models::research::{Research, Researched, Researcher};

/// Answers by model alias, recording which models were asked.
struct Stub {
    by_model: HashMap<String, String>,
    asked: Mutex<Vec<String>>,
}

impl Researcher for Stub {
    fn research(&self, r: Research, _: &Cancel) -> sorcerer_models::Result<Researched> {
        self.asked.lock().unwrap().push(r.model.clone());
        let mut errand = sorcerer_core::ledger::Errand {
            at: chrono::Utc::now(),
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
        match self.by_model.get(&r.model) {
            Some(a) => Ok(Researched {
                answer: a.clone(),
                errand,
                reads: vec![],
            }),
            None => {
                errand.ok = false;
                Err(sorcerer_models::Error::Reply {
                    provider: "stub".into(),
                    why: "no answer".into(),
                })
            }
        }
    }
}

/// Answers every guard with its P(yes), in order.
struct Guards(Vec<Option<f64>>, Mutex<Vec<Vec<Question>>>);

impl Decider for Guards {
    fn ask(&self, qs: &[Question], _: &Cancel) -> sorcerer_models::Result<Vec<Weighed>> {
        self.1.lock().unwrap().push(qs.to_vec());
        Ok(self
            .0
            .iter()
            .map(|p| Weighed {
                answer: match p {
                    Some(p) if *p >= 0.5 => Answer::Yes(Confidence::Known(*p)),
                    Some(p) => Answer::No(Confidence::Known(1.0 - p)),
                    None => Answer::Unsure,
                },
                distribution: vec![],
            })
            .collect())
    }
}

fn reply(verdict: &str, evidence: &str) -> String {
    format!(
        "notes\n```json\n{{\"verdict\": \"{verdict}\", \"why\": \"{verdict} because\", \"evidence\": [{evidence}]}}\n```\n"
    )
}

const GOOD: &str = r#"{"path": "src/orders.py", "unit": "cancel", "line": null}"#;

struct World {
    _d: tempfile::TempDir,
    view: FilesView,
    fences: Fences,
}

fn world() -> World {
    let d = support::git_project(None);
    support::write(
        d.path(),
        "src/orders.py",
        "def cancel(repo, id):\n    order = repo.get(id)\n    return order\n\n\ndef other():\n    pass\n",
    );
    support::write(d.path(), ".env", "SECRET=x\n");
    let mut p = Project::open(d.path()).unwrap();
    let vendored =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../sorcerer-core/tests/grammars/python");
    p.languages
        .override_grammar("python", GrammarSource::Local(vendored));
    let fences = Fences::for_project(&p).unwrap();
    let cache =
        GrammarCache::at(Path::new(env!("CARGO_TARGET_TMPDIR")).join("models-grammar-cache"));
    World {
        view: FilesView::new(Arc::new(p), cache),
        fences,
        _d: d,
    }
}

fn claim() -> Claim {
    Claim {
        statement: "cancel() returns None when the repository has no order with that id.".into(),
        about: vec![],
        kind: ClaimKind::Code,
    }
}

fn stub(answers: &[(&str, String)]) -> Stub {
    Stub {
        by_model: answers
            .iter()
            .map(|(m, a)| (m.to_string(), a.clone()))
            .collect(),
        asked: Mutex::new(vec![]),
    }
}

fn deps<'a>(w: &'a World, r: &'a Stub, d: Option<&'a dyn Decider>) -> CheckerDeps<'a> {
    CheckerDeps::defaults(r, d, &w.fences, &w.view, "scry")
}

#[test]
fn the_prompt_is_olifants() {
    assert_eq!(
        claim_prompt(&claim()),
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/claim-prompt.txt")
        )
        .unwrap()
    );
}

#[test]
fn all_guards_pass_keeps_the_first_verdict() {
    let w = world();
    let r = stub(&[
        ("haiku", reply("right", GOOD)),
        ("sonnet", reply("wrong", GOOD)),
    ]);
    let g = Guards(vec![Some(0.9), Some(0.8), Some(0.7)], Mutex::new(vec![]));
    let c = check(&claim(), &deps(&w, &r, Some(&g)), &Cancel::default()).unwrap();
    assert_eq!((c.verdict, c.escalated), (Verdict::Right, false));
    assert_eq!(*r.asked.lock().unwrap(), vec!["haiku"]);
    let asked = g.1.lock().unwrap();
    let kinds: Vec<_> = asked[0].iter().map(|q| q.kind.as_str()).collect();
    assert_eq!(kinds, vec!["why-fits", "evidence-fits", "verdict-follows"]);
    let ctx: HashMap<_, _> = asked[0][0].context.iter().cloned().collect();
    assert!(
        ctx["evidence"].starts_with("src/orders.py:cancel\ndef cancel(repo, id):"),
        "{}",
        ctx["evidence"]
    );
    assert_eq!(ctx["verdict"], "right: what the sentence states is true");
    assert_eq!(
        c.citations,
        vec![Citation {
            file: PathBuf::from("src/orders.py"),
            lines: Span { start: 1, end: 3 },
            print: c.citations[0].print.clone()
        }]
    );
    assert!(c.citations[0].print.is_some());
}

#[test]
fn one_guard_below_threshold_escalates_to_strong() {
    let w = world();
    let r = stub(&[
        ("haiku", reply("right", GOOD)),
        ("sonnet", reply("wrong", GOOD)),
    ]);
    let g = Guards(vec![Some(0.9), Some(0.59), Some(0.9)], Mutex::new(vec![]));
    let c = check(&claim(), &deps(&w, &r, Some(&g)), &Cancel::default()).unwrap();
    assert_eq!((c.verdict, c.escalated), (Verdict::Wrong, true));
    assert_eq!(*r.asked.lock().unwrap(), vec!["haiku", "sonnet"]);
    assert_eq!(c.errands.len(), 2);
}

#[test]
fn an_unsure_guard_escalates() {
    let w = world();
    let r = stub(&[
        ("haiku", reply("right", GOOD)),
        ("sonnet", reply("right", GOOD)),
    ]);
    let g = Guards(vec![Some(0.9), None, Some(0.9)], Mutex::new(vec![]));
    assert!(
        check(&claim(), &deps(&w, &r, Some(&g)), &Cancel::default())
            .unwrap()
            .escalated
    );
}

#[test]
fn cannot_tell_from_the_first_escalates_without_guards() {
    let w = world();
    let r = stub(&[
        ("haiku", reply("cannot_tell", "")),
        ("sonnet", reply("right", GOOD)),
    ]);
    let g = Guards(vec![Some(0.9), Some(0.9), Some(0.9)], Mutex::new(vec![]));
    let c = check(&claim(), &deps(&w, &r, Some(&g)), &Cancel::default()).unwrap();
    assert_eq!((c.verdict, c.escalated), (Verdict::Right, true));
    assert!(g.1.lock().unwrap().is_empty());
}

#[test]
fn no_decider_checks_on_strong_alone() {
    let w = world();
    let r = stub(&[("sonnet", reply("wrong", GOOD))]);
    let c = check(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
    assert_eq!((c.verdict, c.escalated), (Verdict::Wrong, false));
    assert_eq!(*r.asked.lock().unwrap(), vec!["sonnet"]);
}

#[test]
fn a_citation_into_a_fenced_file_is_cannot_tell() {
    let w = world();
    let r = stub(&[(
        "sonnet",
        reply("right", r#"{"path": ".env", "unit": null, "line": 1}"#),
    )]);
    let c = check(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
    assert_eq!(c.verdict, Verdict::CannotTell);
    assert_eq!(c.why, "cited code not found");
}

#[test]
fn a_unit_the_file_does_not_define_is_cannot_tell() {
    let w = world();
    let r = stub(&[(
        "sonnet",
        reply(
            "right",
            r#"{"path": "src/orders.py", "unit": "refund", "line": null}"#,
        ),
    )]);
    assert_eq!(
        check(&claim(), &deps(&w, &r, None), &Cancel::default())
            .unwrap()
            .verdict,
        Verdict::CannotTell
    );
}

#[test]
fn right_with_no_evidence_is_a_failed_reply() {
    let w = world();
    let r = stub(&[("sonnet", reply("right", ""))]);
    assert!(check(&claim(), &deps(&w, &r, None), &Cancel::default()).is_err());
}

#[test]
fn a_cited_line_is_found_and_a_path_colon_line_is_read() {
    let w = world();
    let r = stub(&[(
        "sonnet",
        reply(
            "right",
            r#"{"path": "src/orders.py:2", "unit": null, "line": null}"#,
        ),
    )]);
    let c = check(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
    assert_eq!(c.verdict, Verdict::Right);
    assert_eq!(c.citations[0].lines, Span { start: 2, end: 2 });
}

#[test]
fn a_line_past_the_end_is_cannot_tell() {
    let w = world();
    let r = stub(&[(
        "sonnet",
        reply(
            "right",
            r#"{"path": "src/orders.py", "unit": null, "line": 99}"#,
        ),
    )]);
    assert_eq!(
        check(&claim(), &deps(&w, &r, None), &Cancel::default())
            .unwrap()
            .verdict,
        Verdict::CannotTell
    );
}

#[test]
fn every_raw_citation_is_kept_with_whether_it_confirmed() {
    let w = world();
    let r = stub(&[(
        "sonnet",
        reply(
            "right",
            &format!(r#"{GOOD}, {{"path": "src/orders.py", "unit": "refund", "line": null}}"#),
        ),
    )]);
    let c = check(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
    assert_eq!(c.verdict, Verdict::CannotTell);
    let got: Vec<(&str, Option<&str>, bool)> = c
        .cited
        .iter()
        .map(|x| (x.raw.path.as_str(), x.raw.unit.as_deref(), x.confirmed))
        .collect();
    assert_eq!(
        got,
        vec![
            ("src/orders.py", Some("cancel"), true),
            ("src/orders.py", Some("refund"), false),
        ]
    );
}
