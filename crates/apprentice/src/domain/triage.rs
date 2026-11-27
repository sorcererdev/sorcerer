//! Triage: what a region is, asked of Jev. Free rules first (a stub is an instruction); the rest
//! is four questions in one batch per region: the kind (question, claim, instruction, aside) and
//! three yes-or-no, whether it is addressed to a reader, wants an answer now, needs a decision.
//! Code decides: a clear kind (0.70) and a clear `addressed` (0.70) act; anything else is
//! nothing, costs nothing more and shows nothing.

use serde::{Deserialize, Serialize};
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Shape, UnsureAnswer, Weighed};

use crate::domain::region::{Region, RegionPlace};

/// How sure Jev must be of the kind for apprentice to act.
pub const KIND_CUT: f64 = 0.70;
/// How sure Jev must be that the region is addressed to a reader.
pub const ADDRESSED_CUT: f64 = 0.70;

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
    /// Jev's most likely kind and its probability, as asked (for the labels).
    pub jev_kind: Option<(Kind, f64)>,
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

/// The four questions about one region; they share its text, so Jev takes them as one request.
pub fn questions_for(region: &Region) -> Vec<Question> {
    let context = vec![
        ("text".to_string(), region.text.clone()),
        ("where".to_string(), where_text(region)),
    ];
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
            "The person wrote or changed this text in their code. What is it?",
            Shape::Choice(vec![
                (
                    "question".into(),
                    "it asks something about the code or the project".into(),
                ),
                (
                    "claim".into(),
                    "it states how the code behaves or should behave".into(),
                ),
                (
                    "instruction".into(),
                    "it asks for code to be written or changed".into(),
                ),
                (
                    "aside".into(),
                    "a note to themselves, documentation, or just code".into(),
                ),
            ]),
        ),
        noul(
            question,
            "addressed",
            "Is the text addressed to a reader who could act on it, rather than to no one?",
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

/// What the four answers (in the order of [`questions_for`]) make: the kind to act on when the
/// kind is at least 0.70 and `addressed` at least 0.70, and never an aside.
///
/// Example: claim 0.91, addressed 0.82 acts on a claim; claim 0.91, addressed 0.40 is nothing.
pub fn verdict_of(answers: &[Weighed]) -> Triaged {
    let jev_kind = answers.first().and_then(chosen_kind);
    let p = |n: usize| answers.get(n).and_then(|weighed| weighed.answer.p_yes());
    let addressed = p(1);
    let clear = |cut: f64, probability: Option<f64>| probability.is_some_and(|p| p >= cut);
    let kind = jev_kind
        .filter(|(kind, probability)| *kind != Kind::Aside && clear(KIND_CUT, Some(*probability)))
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

fn chosen_kind(weighed: &Weighed) -> Option<(Kind, f64)> {
    match &weighed.answer {
        Answer::Chosen(word, Confidence::Known(p)) => Some((Kind::of_word(word)?, *p)),
        Answer::Chosen(word, Confidence::Unknown) => {
            let p = weighed
                .distribution
                .iter()
                .find(|(option, _)| option == word)
                .map_or(0.0, |(_, p)| *p);
            Some((Kind::of_word(word)?, p))
        }
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

    fn answers(kind: &str, p: f64, addressed: f64) -> Vec<Weighed> {
        vec![
            weighed(Answer::Chosen(kind.into(), Confidence::Known(p))),
            weighed(Answer::Yes(Confidence::Known(addressed))),
            weighed(Answer::Yes(Confidence::Known(0.5))),
            weighed(Answer::No(Confidence::Known(0.9))),
        ]
    }

    #[test]
    fn a_clear_kind_addressed_to_a_reader_acts() {
        let triaged = verdict_of(&answers("claim", 0.91, 0.82));
        assert_eq!(triaged.kind, Some(Kind::Claim));
        assert_eq!(triaged.jev_kind, Some((Kind::Claim, 0.91)));
        assert!((triaged.needs_decision.unwrap() - 0.1).abs() < 1e-9);
    }

    #[test]
    fn anything_unclear_or_an_aside_is_nothing() {
        assert_eq!(verdict_of(&answers("claim", 0.91, 0.40)).kind, None);
        assert_eq!(verdict_of(&answers("question", 0.60, 0.95)).kind, None);
        assert_eq!(verdict_of(&answers("aside", 0.99, 0.99)).kind, None);
        assert_eq!(verdict_of(&vec![Weighed::unsure(); 4]).kind, None);
    }

    #[test]
    fn a_stub_is_an_instruction_by_rule_and_four_questions_share_the_text() {
        let stub = Region::new(
            "a.txt",
            Span { start: 1, end: 2 },
            "f():\n  nothing\n",
            RegionPlace::Stub,
            Some("f".into()),
        );
        assert_eq!(free_kind(&stub).unwrap().kind, Some(Kind::Instruction));
        let questions = questions_for(&stub);
        assert_eq!(questions.len(), 4);
        assert!(questions.iter().all(|q| q.context == questions[0].context));
        assert_eq!(questions[0].context[1].1, "an empty body in a.txt, in f");
    }
}
