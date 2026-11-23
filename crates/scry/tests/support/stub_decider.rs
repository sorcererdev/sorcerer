//! A decider that answers what the test tells it to.

use std::sync::Mutex;

use sorcerer_core::Cancel;
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Weighed};
use sorcerer_models::ports::decider::Decider;

type Predicate = Box<dyn Fn(&Question) -> bool + Send + Sync>;

/// The decider's stand-in: the first rule whose kind prefixes the question's
/// kind and whose predicate holds gives the answer; else a certain no.
/// Records every question asked.
pub struct StubDecider {
    rules: Vec<(String, Predicate, Answer)>,
    pub asked: Mutex<Vec<Question>>,
}

impl StubDecider {
    pub fn new() -> StubDecider {
        StubDecider {
            rules: Vec::new(),
            asked: Mutex::new(Vec::new()),
        }
    }

    /// Answers ANSWER to questions of KIND (a prefix) for which WHEN holds.
    pub fn on(
        mut self,
        kind: &str,
        when: impl Fn(&Question) -> bool + Send + Sync + 'static,
        answer: Answer,
    ) -> StubDecider {
        self.rules.push((kind.to_string(), Box::new(when), answer));
        self
    }

    /// A yes/no answer with P(yes) = P.
    pub fn p_yes(p: f64) -> Answer {
        Answer::Yes(Confidence::Known(p))
    }

    /// The context entry NAME of Q, or "".
    pub fn context<'q>(q: &'q Question, name: &str) -> &'q str {
        q.context
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    }

    pub fn asked(&self) -> Vec<Question> {
        self.asked.lock().unwrap().clone()
    }
}

impl Default for StubDecider {
    fn default() -> StubDecider {
        StubDecider::new()
    }
}

impl Decider for StubDecider {
    fn ask(&self, qs: &[Question], _cancel: &Cancel) -> sorcerer_models::Result<Vec<Weighed>> {
        self.asked.lock().unwrap().extend(qs.iter().cloned());
        Ok(qs
            .iter()
            .map(|q| {
                let answer = self
                    .rules
                    .iter()
                    .find(|(k, when, _)| q.kind.starts_with(k.as_str()) && when(q))
                    .map(|(_, _, a)| a.clone())
                    .unwrap_or(Answer::No(Confidence::Known(1.0)));
                Weighed {
                    answer,
                    distribution: Vec::new(),
                }
            })
            .collect())
    }
}
