//! Triage: what a region is, asked of Jev. Free rules first (a stub is an instruction); the rest
//! is four questions in one batch per region, with the region's context (the lines around it,
//! its section, the session's changes: `triage_context`): the kind (question, claim,
//! instruction, aside) and three yes-or-no, whether the person asks for something now, wants an
//! answer now, needs a decision. Code decides: when the kinds other than aside together reach
//! 0.25 and the ask reaches 0.12, the most likely of them acts; anything else is nothing,
//! costs nothing more and shows nothing. The cuts were set on `corpus/apprentice/triage`.

use serde::{Deserialize, Serialize};
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Shape, UnsureAnswer, Weighed};

use crate::domain::region::{Region, RegionPlace};
use crate::domain::triage_context::TriageContext;

/// How likely Jev must find it that the region is not an aside (a question, a claim and an
/// instruction together) for apprentice to act. Jev puts most of its weight on aside for
/// anything in a file, asks included, so the cut is low: on the corpus no ordinary text reached
/// 0.25 that the ask question did not stop.
pub const KIND_CUT: f64 = 0.25;
/// How likely Jev must find it that the person asks for something now. Jev keeps this
/// question's answers low; it stops notes for later (0.06 to 0.10 on the corpus) where asks
/// sit at 0.14 and above.
pub const ADDRESSED_CUT: f64 = 0.12;

/// What a region is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// The person asks something: answered on save.
    Question,
    /// The person states how the code is or should be: checked on save.
    Claim,
    /// The person asks for something to be done: offered, never run unasked.
    Instruction,
    /// A note to themselves, or code: nothing.
    Aside,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Question => "question",
            Kind::Claim => "claim",
            Kind::Instruction => "instruction",
            Kind::Aside => "aside",
        }
    }

    pub fn of_word(word: &str) -> Option<Kind> {
        [Kind::Question, Kind::Claim, Kind::Instruction, Kind::Aside]
            .into_iter()
            .find(|kind| kind.word() == word)
    }
}

/// Jev's answers about one region, and what code made of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Triaged {
    /// The kind to act on; `None` when Jev was not clear enough (or said aside).
    pub kind: Option<Kind>,
    /// Jev's most likely kind other than aside and how likely Jev finds it that the region is
    /// not an aside, as asked (for the labels).
    pub jev_kind: Option<(Kind, f64)>,
    /// How likely Jev finds it that the person asks for something now (an answer, a check,
    /// work done) rather than writing the file's content or a note for later.
    pub addressed: Option<f64>,
    pub wants_now: Option<f64>,
    pub needs_decision: Option<f64>,
    /// Set by a free rule, not Jev.
    pub by_rule: bool,
}

/// The kind a free rule gives, with no call: a stub is an instruction.
pub fn free_kind(region: &Region) -> Option<Triaged> {
    (region.place == RegionPlace::Stub).then_some(Triaged {
        kind: Some(Kind::Instruction),
        jev_kind: None,
        addressed: None,
        wants_now: None,
        needs_decision: None,
        by_rule: true,
    })
}

/// The four questions about one region; they share its text and context, so Jev takes them
/// as one request.
pub fn questions_for(region: &Region, around: &TriageContext) -> Vec<Question> {
    let context = state_of(region, around);
    let question = |kind: &str, instructions: &str, shape: Shape| Question {
        kind: kind.to_string(),
        instructions: instructions.to_string(),
        shape,
        context: context.clone(),
        unsure: UnsureAnswer::NotAllowed,
    };
    vec![
        question(
            "kind",
            "The person wrote or changed `text` in their project. What is it?",
            Shape::Choice(vec![
                (
                    "question".into(),
                    "it asks something about the code, the text or the project".into(),
                ),
                (
                    "claim".into(),
                    "it states how the code behaves, to be checked".into(),
                ),
                (
                    "instruction".into(),
                    "it asks for something to be written, filled in or changed (code, tests or \
                     text), or it is a placeholder such as TBD standing where content is missing"
                        .into(),
                ),
                (
                    "aside".into(),
                    "it is the file's own content: code, documentation, a heading or an \
                     explanation"
                        .into(),
                ),
            ]),
        ),
        question(
            "addressed",
            "Does the person want their coding assistant to answer, check or do what `text` \
             says, now?",
            Shape::Noul {
                yes: "yes: it is a request, a question or a claim to check, for now".into(),
                no: "no: it is the file's own content, or a note left for later".into(),
            },
        ),
        noul(
            question,
            "wants_now",
            "Does the person want an answer now, rather than leaving a note for later?",
        ),
        noul(
            question,
            "needs_decision",
            "Does the text ask for a choice between options?",
        ),
    ]
}

/// Jev's state for a region: its text, where it is, the lines around it, its section (for a
/// paragraph under a heading) and the session's changes (when there are any).
fn state_of(region: &Region, around: &TriageContext) -> Vec<(String, String)> {
    let named = |name: &str, text: &str| (name.to_string(), text.to_string());
    let mut state = vec![
        named("text", &region.text),
        named("where", &where_text(region)),
        named("around", &around.around),
    ];
    state.extend(
        around
            .section
            .as_deref()
            .map(|section| named("section", section)),
    );
    if !around.session_changes.is_empty() {
        state.push(named("session_changes", &around.session_changes));
    }
    state
}

fn noul(
    question: impl Fn(&str, &str, Shape) -> Question,
    kind: &str,
    instructions: &str,
) -> Question {
    question(
        kind,
        instructions,
        Shape::Noul {
            yes: "yes".into(),
            no: "no".into(),
        },
    )
}

/// Where the text is, for Jev: `a comment in src/cancel_booking.py, in CancelBooking.handle`.
fn where_text(region: &Region) -> String {
    let what = match region.place {
        RegionPlace::Comment => "a comment",
        RegionPlace::Prose => "a paragraph",
        RegionPlace::Stub => "an empty body",
        RegionPlace::Hunk => "changed code",
    };
    match &region.unit {
        Some(unit) => format!("{what} in {}, in {unit}", region.file),
        None => format!("{what} in {}", region.file),
    }
}

/// What the four answers (in the order of [`questions_for`]) make: when the kinds other than
/// aside together reach 0.25 and the ask reaches 0.12, the most likely of them.
///
/// Example: aside 0.60, instruction 0.35, question 0.05, ask 0.20 acts on an instruction; the
/// same kinds with ask 0.08 (a note for later) is nothing.
pub fn verdict_of(answers: &[Weighed]) -> Triaged {
    let jev_kind = answers.first().and_then(likeliest_ask);
    let p = |n: usize| answers.get(n).and_then(|weighed| weighed.answer.p_yes());
    let addressed = p(1);
    let clear = |cut: f64, probability: Option<f64>| probability.is_some_and(|p| p >= cut);
    let kind = jev_kind
        .filter(|(_, not_aside)| clear(KIND_CUT, Some(*not_aside)))
        .filter(|_| clear(ADDRESSED_CUT, addressed))
        .map(|(kind, _)| kind);
    Triaged {
        kind,
        jev_kind,
        addressed,
        wants_now: p(2),
        needs_decision: p(3),
        by_rule: false,
    }
}

/// The most likely kind other than aside, with how likely Jev finds it that the region is not
/// an aside; from the choice's distribution, else (no distribution) from its confidence.
fn likeliest_ask(weighed: &Weighed) -> Option<(Kind, f64)> {
    let asks: Vec<(Kind, f64)> = weighed
        .distribution
        .iter()
        .filter_map(|(word, p)| Some((Kind::of_word(word)?, *p)))
        .filter(|(kind, _)| *kind != Kind::Aside)
        .collect();
    if let Some(&(kind, _)) = asks.iter().max_by(|x, y| x.1.total_cmp(&y.1)) {
        return Some((kind, asks.iter().map(|(_, p)| p).sum()));
    }
    match &weighed.answer {
        Answer::Chosen(word, Confidence::Known(p)) => Kind::of_word(word)
            .filter(|kind| *kind != Kind::Aside)
            .map(|kind| (kind, *p)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use sorcerer_core::domain::unit::Span;

    use super::*;

    fn weighed(answer: Answer) -> Weighed {
        Weighed {
            answer,
            distribution: Vec::new(),
        }
    }

    /// Jev's four answers: the kind's distribution (aside, instruction, question), and the ask.
    fn answers(aside: f64, instruction: f64, question: f64, ask: f64) -> Vec<Weighed> {
        let likeliest = if aside >= instruction {
            "aside"
        } else {
            "instruction"
        };
        vec![
            Weighed {
                answer: Answer::Chosen(likeliest.into(), Confidence::Known(0.8)),
                distribution: vec![
                    ("aside".into(), aside),
                    ("instruction".into(), instruction),
                    ("question".into(), question),
                    ("claim".into(), 0.0),
                ],
            },
            weighed(Answer::Yes(Confidence::Known(ask))),
            weighed(Answer::Yes(Confidence::Known(0.5))),
            weighed(Answer::No(Confidence::Known(0.9))),
        ]
    }

    #[test]
    fn the_likeliest_ask_acts_when_the_asks_together_and_the_ask_clear_their_cuts() {
        let triaged = verdict_of(&answers(0.60, 0.35, 0.05, 0.20));
        assert_eq!(triaged.kind, Some(Kind::Instruction));
        let (kind, not_aside) = triaged.jev_kind.unwrap();
        assert_eq!(kind, Kind::Instruction);
        assert!((not_aside - 0.40).abs() < 1e-9);
        assert!((triaged.needs_decision.unwrap() - 0.1).abs() < 1e-9);
    }

    #[test]
    fn a_note_for_later_an_aside_or_no_answer_is_nothing() {
        assert_eq!(verdict_of(&answers(0.60, 0.35, 0.05, 0.08)).kind, None);
        assert_eq!(verdict_of(&answers(0.90, 0.06, 0.04, 0.50)).kind, None);
        assert_eq!(verdict_of(&vec![Weighed::unsure(); 4]).kind, None);
        let aside_without_distribution = vec![
            weighed(Answer::Chosen("aside".into(), Confidence::Known(0.4))),
            weighed(Answer::Yes(Confidence::Known(0.9))),
        ];
        assert_eq!(verdict_of(&aside_without_distribution).kind, None);
    }

    #[test]
    fn a_stub_is_an_instruction_by_rule_and_four_questions_share_the_text_and_context() {
        let stub = Region::new(
            "a.txt",
            Span { start: 1, end: 2 },
            "f():\n  nothing\n",
            RegionPlace::Stub,
            Some("f".into()),
        );
        assert_eq!(free_kind(&stub).unwrap().kind, Some(Kind::Instruction));
        let context = TriageContext {
            around: "> f():\n>   nothing\n".into(),
            section: None,
            session_changes: String::new(),
        };
        let questions = questions_for(&stub, &context);
        assert_eq!(questions.len(), 4);
        assert!(questions.iter().all(|q| q.context == questions[0].context));
        let names: Vec<&str> = questions[0]
            .context
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["text", "where", "around"]);
        assert_eq!(questions[0].context[1].1, "an empty body in a.txt, in f");
    }
}
