//! Jev and the researcher, as doubles: Jev answers each question by its kind from a table, the
//! researcher gives one fixed reply and records nothing.

use std::sync::Mutex;

use chrono::Utc;
use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Weighed};
use sorcerer_models::domain::research::{Research, Researched};
use sorcerer_models::ports::decider::Decider;
use sorcerer_models::ports::researcher::Researcher;

/// Answers `kind` with the chosen kind at its probability, `addressed` with its probability,
/// and every other yes-or-no question yes at 0.9 (the claim checker's guards included).
pub struct StubJev {
    pub kind: &'static str,
    pub kind_p: f64,
    pub addressed: f64,
    pub asked: Mutex<Vec<Question>>,
}

impl StubJev {
    pub fn saying(kind: &'static str, kind_p: f64, addressed: f64) -> StubJev {
        StubJev {
            kind,
            kind_p,
            addressed,
            asked: Mutex::new(Vec::new()),
        }
    }
}

impl Decider for StubJev {
    fn ask(&self, questions: &[Question], _: &Cancel) -> sorcerer_models::Result<Vec<Weighed>> {
        self.asked.lock().unwrap().extend(questions.iter().cloned());
        Ok(questions
            .iter()
            .map(|question| {
                let answer = match question.kind.as_str() {
                    "kind" => Answer::Chosen(self.kind.into(), Confidence::Known(self.kind_p)),
                    "addressed" => Answer::Yes(Confidence::Known(self.addressed)),
                    _ => Answer::Yes(Confidence::Known(0.9)),
                };
                Weighed {
                    answer,
                    distribution: Vec::new(),
                }
            })
            .collect())
    }
}

/// Gives the same reply to every research, at $0.02.
pub struct StubResearcher {
    pub reply: String,
    pub prompts: Mutex<Vec<String>>,
}

impl StubResearcher {
    pub fn replying(reply: &str) -> StubResearcher {
        StubResearcher {
            reply: reply.into(),
            prompts: Mutex::new(Vec::new()),
        }
    }
}

impl Researcher for StubResearcher {
    fn research(&self, research: Research, _: &Cancel) -> sorcerer_models::Result<Researched> {
        self.prompts.lock().unwrap().push(research.prompt.clone());
        Ok(Researched {
            answer: self.reply.clone(),
            errand: Errand {
                at: Utc::now(),
                product: research.product,
                verb: research.verb,
                model: research.model,
                provider: "stub".into(),
                cost_usd: Some(0.02),
                input_tokens: 0,
                output_tokens: 0,
                seconds: 1.0,
                ok: true,
                note: None,
            },
            reads: Vec::new(),
        })
    }
}
