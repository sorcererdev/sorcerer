//! Recorded answers for tests and the eval: one file per question,
//! `<dir>/<question_key>.json`, holding Jev's answer object for it. A miss
//! is an error, unless a live Jev is given to record it.

use std::path::PathBuf;

use serde_json::json;
use sorcerer_core::Cancel;

use super::jev::{Jev, jev_answers, question_key};
use super::{Decider, Question, Weighed};
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

    /// Records a missing answer by asking LIVE, one question a call.
    pub fn recording_with(mut self, live: Jev) -> RecordedDecider {
        self.record_with = Some(live);
        self
    }
}

impl Decider for RecordedDecider {
    fn ask(&self, qs: &[Question], cancel: &Cancel) -> Result<Vec<Weighed>> {
        let mut out = Vec::new();
        for q in qs {
            let file = self.dir.join(format!("{}.json", question_key(q)));
            let answer = match std::fs::read_to_string(&file) {
                Ok(text) => serde_json::from_str(&text).map_err(|e| Error::Reply {
                    provider: "recorded".into(),
                    why: format!("{}: {e}", file.display()),
                })?,
                Err(_) => {
                    let Some(live) = &self.record_with else {
                        return Err(Error::Reply {
                            provider: "recorded".into(),
                            why: format!(
                                "no recording for question {} ({})",
                                q.kind,
                                file.display()
                            ),
                        });
                    };
                    let a =
                        live.ask_raw(std::slice::from_ref(q), cancel)?["answers"][&q.kind].clone();
                    std::fs::create_dir_all(&self.dir)
                        .map_err(|e| sorcerer_core::Error::io(&self.dir, e))?;
                    std::fs::write(&file, serde_json::to_string_pretty(&a).unwrap_or_default())
                        .map_err(|e| sorcerer_core::Error::io(&file, e))?;
                    a
                }
            };
            let raw = json!({"answers": {q.kind.clone(): answer}}).to_string();
            out.extend(jev_answers(std::slice::from_ref(q), raw.as_bytes()));
        }
        Ok(out)
    }
}
