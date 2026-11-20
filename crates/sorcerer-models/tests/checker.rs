#![allow(clippy::too_many_lines)]
mod support;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::{GrammarSource, Languages};
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Span;
use sorcerer_models::domain::claim::{Citation, Claim, ClaimKind, Verdict};
use sorcerer_models::domain::claim_prompt::claim_prompt;
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Weighed};
use sorcerer_models::domain::research::{Research, Researched};
use sorcerer_models::ports::decider::Decider;
use sorcerer_models::ports::researcher::Researcher;
use sorcerer_models::usecases::check_claim::{CheckClaimDeps, check_claim};

/// Answers by model alias, recording which models were asked.
struct Stub {
    by_model: HashMap<String, String>,
    asked: Mutex<Vec<String>>,
    prompts: Mutex<Vec<String>>,
}

impl Researcher for Stub {
    fn research(&self, r: Research, _: &Cancel) -> sorcerer_models::Result<Researched> {
        self.asked.lock().unwrap().push(r.model.clone());
        self.prompts.lock().unwrap().push(r.prompt.clone());
        let mut errand = sorcerer_core::domain::errand::Errand {
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
        prompts: Mutex::new(vec![]),
    }
}

fn deps<'a>(w: &'a World, r: &'a Stub, d: Option<&'a dyn Decider>) -> CheckClaimDeps<'a> {
    CheckClaimDeps::defaults(r, d, &w.fences, &w.view, "scry")
}

/// The measured prompt, the unit worded neutrally (the claim points at no
/// file) and the line asked for alongside a unit.
#[test]
fn the_prompt_matches_the_golden() {
    assert_eq!(
        claim_prompt(&claim(), None),
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
    let c = check_claim(&claim(), &deps(&w, &r, Some(&g)), &Cancel::default()).unwrap();
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
    let c = check_claim(&claim(), &deps(&w, &r, Some(&g)), &Cancel::default()).unwrap();
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
        check_claim(&claim(), &deps(&w, &r, Some(&g)), &Cancel::default())
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
    let c = check_claim(&claim(), &deps(&w, &r, Some(&g)), &Cancel::default()).unwrap();
    assert_eq!((c.verdict, c.escalated), (Verdict::Right, true));
    assert!(g.1.lock().unwrap().is_empty());
}

#[test]
fn no_decider_checks_on_strong_alone() {
    let w = world();
    let r = stub(&[("sonnet", reply("wrong", GOOD))]);
    let c = check_claim(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
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
    let c = check_claim(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
    assert_eq!(c.verdict, Verdict::CannotTell);
    assert_eq!(c.why, "cited code not found");
}

#[test]
fn a_unit_the_file_does_not_define_with_no_line_is_cannot_tell() {
    let w = world();
    let r = stub(&[(
        "sonnet",
        reply(
            "right",
            r#"{"path": "src/orders.py", "unit": "refund", "line": null}"#,
        ),
    )]);
    assert_eq!(
        check_claim(&claim(), &deps(&w, &r, None), &Cancel::default())
            .unwrap()
            .verdict,
        Verdict::CannotTell
    );
}

#[test]
fn right_with_no_evidence_is_a_failed_reply() {
    let w = world();
    let r = stub(&[("sonnet", reply("right", ""))]);
    assert!(check_claim(&claim(), &deps(&w, &r, None), &Cancel::default()).is_err());
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
    let c = check_claim(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
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
        check_claim(&claim(), &deps(&w, &r, None), &Cancel::default())
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
    let c = check_claim(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
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

fn claim_about(files: &[&str]) -> Claim {
    Claim {
        about: files
            .iter()
            .map(|f| Citation {
                file: PathBuf::from(f),
                lines: Span { start: 1, end: 1 },
                print: None,
            })
            .collect(),
        ..claim()
    }
}

fn shipped() -> Languages {
    Languages::assemble(tempfile::TempDir::new().unwrap().path(), &[]).unwrap()
}

#[test]
fn a_claim_on_a_rust_file_names_units_the_rust_way() {
    let ls = shipped();
    let p = claim_prompt(&claim_about(&["src/checker.rs"]), Some(&ls));
    assert!(p.contains("(Type::method)"), "{p}");
    assert!(!p.contains("Class.method"), "{p}");
}

#[test]
fn a_claim_on_a_python_file_names_units_the_python_way() {
    let ls = shipped();
    let p = claim_prompt(&claim_about(&["src/orders.py"]), Some(&ls));
    assert!(p.contains("(Class.method)"), "{p}");
    assert!(!p.contains("Type::method"), "{p}");
}

#[test]
fn a_claim_on_a_file_no_language_covers_names_units_neutrally() {
    let ls = shipped();
    for p in [
        claim_prompt(&claim_about(&["notes.txt"]), Some(&ls)),
        claim_prompt(&claim_about(&["src/orders.py"]), None),
        claim_prompt(&claim(), Some(&ls)),
    ] {
        assert!(p.contains("a function, class or method name"), "{p}");
        assert!(
            !p.contains("Class.method") && !p.contains("Type::method"),
            "{p}"
        );
    }
}

#[test]
fn a_claim_across_languages_names_each_ones_units() {
    let ls = shipped();
    let p = claim_prompt(
        &claim_about(&["src/orders.py", "src/checker.rs", "src/more.py"]),
        Some(&ls),
    );
    assert!(
        p.contains("(Class.method)") && p.contains("(Type::method)"),
        "{p}"
    );
    assert!(p.contains("`src/orders.py`, `src/more.py`"), "{p}");
}

#[test]
fn the_checker_asks_in_the_claims_language() {
    let w = world();
    let r = stub(&[("sonnet", reply("right", GOOD))]);
    let ls = shipped();
    let mut d = deps(&w, &r, None);
    d.languages = Some(&ls);
    check_claim(&claim_about(&["src/orders.py"]), &d, &Cancel::default()).unwrap();
    assert!(r.prompts.lock().unwrap()[0].contains("(Class.method)"));
}

#[test]
fn an_unknown_unit_with_a_good_line_is_confirmed_on_the_line() {
    let w = world();
    let r = stub(&[(
        "sonnet",
        reply(
            "right",
            r#"{"path": "src/orders.py", "unit": "Orders::cancel_it", "line": 2}"#,
        ),
    )]);
    let c = check_claim(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
    assert_eq!(c.verdict, Verdict::Right);
    assert_eq!(c.citations[0].lines, Span { start: 2, end: 2 });
    assert!(c.citations[0].print.is_some());
    assert!(c.cited[0].confirmed && c.cited[0].unit_missed);
}

#[test]
fn a_found_unit_is_not_marked_missed() {
    let w = world();
    let r = stub(&[("sonnet", reply("right", GOOD))]);
    let c = check_claim(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
    assert!(c.cited[0].confirmed && !c.cited[0].unit_missed);
}

#[test]
fn an_unknown_unit_with_a_line_past_the_end_is_cannot_tell() {
    let w = world();
    let r = stub(&[(
        "sonnet",
        reply(
            "right",
            r#"{"path": "src/orders.py", "unit": "refund", "line": 99}"#,
        ),
    )]);
    let c = check_claim(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
    assert_eq!(c.verdict, Verdict::CannotTell);
    assert!(!c.cited[0].confirmed);
}

#[test]
fn a_fenced_file_with_a_good_unit_is_cannot_tell() {
    let w = world();
    // a Python file the fences hold back (`.env*`), defining the unit cited
    support::write(w._d.path(), ".env.py", "def cancel():\n    pass\n");
    let r = stub(&[(
        "sonnet",
        reply(
            "right",
            r#"{"path": ".env.py", "unit": "cancel", "line": 1}"#,
        ),
    )]);
    let c = check_claim(&claim(), &deps(&w, &r, None), &Cancel::default()).unwrap();
    assert_eq!(c.verdict, Verdict::CannotTell);
}
