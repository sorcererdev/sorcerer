//! A decider double: every question gets the same choice distribution, and the questions are
//! kept for the test to read.

use std::sync::Mutex;

use sorcerer_core::Cancel;
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Weighed};
use sorcerer_models::ports::decider::Decider;

pub struct SameAnswer {
    distribution: Vec<(String, f64)>,
    pub asked: Mutex<Vec<Question>>,
}

impl SameAnswer {
    /// Answers each question with these options' probabilities, the likeliest chosen.
    pub fn choosing(distribution: &[(&str, f64)]) -> SameAnswer {
        SameAnswer {
            distribution: distribution
                .iter()
                .map(|(name, p)| (name.to_string(), *p))
                .collect(),
            asked: Mutex::new(Vec::new()),
        }
    }

    pub fn questions(&self) -> Vec<Question> {
        self.asked.lock().unwrap().clone()
    }
}

impl Decider for SameAnswer {
    fn ask(&self, questions: &[Question], _: &Cancel) -> sorcerer_models::Result<Vec<Weighed>> {
        self.asked.lock().unwrap().extend(questions.iter().cloned());
        let (likeliest, p) = self
            .distribution
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .cloned()
            .unwrap();
        Ok(questions
            .iter()
            .map(|_| Weighed {
                answer: Answer::Chosen(likeliest.clone(), Confidence::Known(p)),
                distribution: self.distribution.clone(),
            })
            .collect())
    }
}
