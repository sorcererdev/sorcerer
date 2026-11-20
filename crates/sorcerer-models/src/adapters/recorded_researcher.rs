//! Recorded research for tests and the eval: answers from
//! `<dir>/<key>.json` (`{"answer": …, "cost_usd": …}`), keyed by the model
//! and the prompt. A missing recording is an error, unless a live
//! researcher is given to record it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::fnv::fnv64_hex;

use crate::domain::research::{Research, Researched};
use crate::ports::researcher::Researcher;
use crate::{Error, Result};

pub struct RecordedResearcher {
    dir: PathBuf,
    record_with: Option<Arc<dyn Researcher>>,
}

/// A research's identity in a recording: sixteen hex digits of an FNV-1a 64 hash of the
/// model, a newline, and the prompt. The same prompt to another model is another recording.
pub fn recording_key(model: &str, prompt: &str) -> String {
    fnv64_hex(format!("{model}\n{prompt}").as_bytes())
}

impl RecordedResearcher {
    pub fn new(dir: PathBuf) -> RecordedResearcher {
        RecordedResearcher {
            dir,
            record_with: None,
        }
    }

    /// Fills in a missing recording by asking the live researcher once and saving its
    /// answer and cost, so the next run finds it.
    pub fn recording_with(mut self, live: Arc<dyn Researcher>) -> RecordedResearcher {
        self.record_with = Some(live);
        self
    }

    fn replay(&self, r: &Research, text: &str, file: &Path) -> Result<Researched> {
        let recording: Value = serde_json::from_str(text).map_err(|e| Error::Reply {
            provider: "recorded".into(),
            why: format!("{}: {e}", file.display()),
        })?;
        Ok(Researched {
            answer: recording["answer"].as_str().unwrap_or_default().to_string(),
            errand: errand_for(r, recording["cost_usd"].as_f64()),
            reads: Vec::new(),
        })
    }

    fn record_live(&self, r: Research, file: &Path, cancel: &Cancel) -> Result<Researched> {
        let Some(live) = &self.record_with else {
            return Err(Error::Reply {
                provider: "recorded".into(),
                why: format!(
                    "no recording {} ({} for {})",
                    recording_key(&r.model, &r.prompt),
                    file.display(),
                    r.verb
                ),
            });
        };
        let researched = live.research(r, cancel)?;
        let recording =
            json!({"answer": researched.answer, "cost_usd": researched.errand.cost_usd});
        std::fs::create_dir_all(&self.dir).map_err(|e| sorcerer_core::Error::io(&self.dir, e))?;
        std::fs::write(
            file,
            serde_json::to_string_pretty(&recording).unwrap_or_default(),
        )
        .map_err(|e| sorcerer_core::Error::io(file, e))?;
        Ok(researched)
    }
}

impl Researcher for RecordedResearcher {
    /// The recorded answer and cost for this model and prompt; when there is none, the
    /// live researcher's, recorded now, or an error naming the missing file.
    fn research(&self, r: Research, cancel: &Cancel) -> Result<Researched> {
        let file = self
            .dir
            .join(format!("{}.json", recording_key(&r.model, &r.prompt)));
        match std::fs::read_to_string(&file) {
            Ok(text) => self.replay(&r, &text, &file),
            Err(_) => self.record_live(r, &file, cancel),
        }
    }
}

/// The ledger line a recording stands for: who asked and what it cost, nothing more.
fn errand_for(r: &Research, cost: Option<f64>) -> Errand {
    Errand {
        at: chrono::Utc::now(),
        product: r.product.clone(),
        verb: r.verb.clone(),
        model: r.model.clone(),
        provider: "recorded".into(),
        cost_usd: cost,
        input_tokens: 0,
        output_tokens: 0,
        seconds: 0.0,
        ok: true,
        note: None,
    }
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::research::Tool;
    use sorcerer_core::Cancel;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn research(prompt: &str) -> Research {
        Research {
            product: "scry".into(),
            verb: "find".into(),
            prompt: prompt.into(),
            model: "sonnet".into(),
            tools: vec![Tool::Read],
            cap_usd: 0.1,
            timeout: Duration::from_secs(1),
        }
    }

    struct Counting(AtomicUsize);

    impl Researcher for Counting {
        fn research(&self, r: Research, _: &Cancel) -> crate::Result<Researched> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Researched {
                answer: format!("live: {}", r.prompt),
                errand: errand_for(&r, Some(0.02)),
                reads: vec![],
            })
        }
    }

    #[test]
    fn recorded_answers_by_key_and_misses_loudly() {
        let d = tempfile::TempDir::new().unwrap();
        let key = recording_key("sonnet", "p1");
        std::fs::write(
            d.path().join(format!("{key}.json")),
            r#"{"answer": "recorded", "cost_usd": 0.03}"#,
        )
        .unwrap();
        let r = RecordedResearcher::new(d.path().into());
        let got = r.research(research("p1"), &Cancel::default()).unwrap();
        assert_eq!(
            (
                got.answer.as_str(),
                got.errand.cost_usd,
                got.errand.provider.as_str()
            ),
            ("recorded", Some(0.03), "recorded")
        );
        let e = r
            .research(research("p2"), &Cancel::default())
            .unwrap_err()
            .to_string();
        assert!(e.contains("no recording"), "{e}");
    }

    #[test]
    fn recorded_with_a_live_researcher_records_once() {
        let d = tempfile::TempDir::new().unwrap();
        let live = Arc::new(Counting(AtomicUsize::new(0)));
        let r = RecordedResearcher::new(d.path().into()).recording_with(live.clone());
        assert_eq!(
            r.research(research("p"), &Cancel::default())
                .unwrap()
                .answer,
            "live: p"
        );
        assert_eq!(
            r.research(research("p"), &Cancel::default())
                .unwrap()
                .answer,
            "live: p"
        );
        assert_eq!(live.0.load(Ordering::SeqCst), 1);
    }
}
