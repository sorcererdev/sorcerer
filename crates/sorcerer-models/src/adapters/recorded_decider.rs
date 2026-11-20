//! Recorded answers for tests and the eval: one file per question,
//! `<dir>/<question_key>.json`, holding Jev's answer object for it. A miss
//! is an error, unless a live Jev is given to record it.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sorcerer_core::Cancel;

use crate::adapters::jev::{Jev, jev_answers, question_key};
use crate::domain::decide::{Question, Weighed};
use crate::ports::decider::Decider;
use crate::{Error, Result};

pub struct RecordedDecider {
    dir: PathBuf,
    record_with: Option<Jev>,
}

impl RecordedDecider {
    pub fn new(dir: PathBuf) -> RecordedDecider {
        RecordedDecider {
            dir,
            record_with: None,
        }
    }

    /// Fills in a missing recording by asking the live Jev, one question per call, and
    /// saving its answer, so the next run finds it.
    pub fn recording_with(mut self, live: Jev) -> RecordedDecider {
        self.record_with = Some(live);
        self
    }

    /// The question's recorded answer, recorded now when it is missing and a live Jev
    /// was given.
    fn recorded_answer(&self, q: &Question, cancel: &Cancel) -> Result<Value> {
        let file = self.dir.join(format!("{}.json", question_key(q)));
        match std::fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| Error::Reply {
                provider: "recorded".into(),
                why: format!("{}: {e}", file.display()),
            }),
            Err(_) => self.record_live(q, &file, cancel),
        }
    }

    fn record_live(&self, q: &Question, file: &Path, cancel: &Cancel) -> Result<Value> {
        let Some(live) = &self.record_with else {
            return Err(Error::Reply {
                provider: "recorded".into(),
                why: format!("no recording for question {} ({})", q.kind, file.display()),
            });
        };
        let answer = live.ask_raw(std::slice::from_ref(q), cancel)?["answers"][&q.kind].clone();
        self.save(file, &answer)?;
        Ok(answer)
    }

    fn save(&self, file: &Path, answer: &Value) -> Result<()> {
        std::fs::create_dir_all(&self.dir).map_err(|e| sorcerer_core::Error::io(&self.dir, e))?;
        std::fs::write(
            file,
            serde_json::to_string_pretty(answer).unwrap_or_default(),
        )
        .map_err(|e| sorcerer_core::Error::io(file, e))?;
        Ok(())
    }
}

impl Decider for RecordedDecider {
    /// Each question's recorded answer, read the way a live reply from Jev is read, so a
    /// recording that does not fit its question is unsure, as it would be live.
    fn ask(&self, qs: &[Question], cancel: &Cancel) -> Result<Vec<Weighed>> {
        qs.iter()
            .map(|q| {
                let answer = self.recorded_answer(q, cancel)?;
                let reply = json!({"answers": {q.kind.clone(): answer}}).to_string();
                Ok(jev_answers(std::slice::from_ref(q), reply.as_bytes()))
            })
            .collect::<Result<Vec<_>>>()
            .map(|per_question| per_question.into_iter().flatten().collect())
    }
}
