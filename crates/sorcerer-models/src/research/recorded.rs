//! Recorded research for tests and the eval: answers from
//! `<dir>/<key>.json` (`{"answer": …, "cost_usd": …}`), keyed by the model
//! and the prompt. A missing recording is an error, unless a live
//! researcher is given to record it.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use sorcerer_core::Cancel;
use sorcerer_core::fnv::fnv64_hex;

use super::{Research, Researched, Researcher, errand_for};
use crate::{Error, Result};

pub struct RecordedResearcher {
    dir: PathBuf,
    record_with: Option<Arc<dyn Researcher>>,
}

/// Sixteen hex digits of FNV-1a 64 over `MODEL + "\n" + PROMPT`.
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

    /// Records a missing answer by asking LIVE (once).
    pub fn recording_with(mut self, live: Arc<dyn Researcher>) -> RecordedResearcher {
        self.record_with = Some(live);
        self
    }
}

impl Researcher for RecordedResearcher {
    fn research(&self, r: Research, cancel: &Cancel) -> Result<Researched> {
        let key = recording_key(&r.model, &r.prompt);
        let file = self.dir.join(format!("{key}.json"));
        if let Ok(text) = std::fs::read_to_string(&file) {
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| Error::Reply {
                provider: "recorded".into(),
                why: format!("{}: {e}", file.display()),
            })?;
            let answer = v["answer"].as_str().unwrap_or_default().to_string();
            return Ok(Researched {
                answer,
                errand: errand_for(&r, v["cost_usd"].as_f64()),
                reads: Vec::new(),
            });
        }
        let Some(live) = &self.record_with else {
            return Err(Error::Reply {
                provider: "recorded".into(),
                why: format!("no recording {key} ({} for {})", file.display(), r.verb),
            });
        };
        let got = live.research(r, cancel)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| sorcerer_core::Error::io(&self.dir, e))?;
        let body = json!({"answer": got.answer, "cost_usd": got.errand.cost_usd});
        std::fs::write(
            &file,
            serde_json::to_string_pretty(&body).unwrap_or_default(),
        )
        .map_err(|e| sorcerer_core::Error::io(&file, e))?;
        Ok(got)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::{Research, Researched, Researcher, Tool};
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
