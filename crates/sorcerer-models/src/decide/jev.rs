//! Jev (typesafe.ai): `POST https://api.typesafe.ai/v1/systemone` with one
//! `state` map and a `questions` map, the model pinned. Ported from
//! olifant's `Decide.hs` (`jevBody`, `jevWeighed`, `jevDecider`). A reply
//! that fails validation, or a call that fails, is `Unsure` for the whole
//! batch: a decider never guesses.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use sorcerer_core::Cancel;
use sorcerer_core::domain::config::DecideConfig;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::ledger::Ledger;

use super::{Answer, Confidence, Decider, Question, Shape, Weighed};
use crate::provider::{Post, post_json};
use crate::{Error, Result};

pub const MODEL: &str = "jev-1.14.0";
pub const URL: &str = "https://api.typesafe.ai";

/// The body of a batch, without the model.
pub fn jev_body(qs: &[Question]) -> Result<Value> {
    let bad = |why: String| Error::Reply {
        provider: "jev".into(),
        why,
    };
    let mut state = Map::new();
    let mut questions = Map::new();
    for q in qs {
        for (k, v) in &q.context {
            match state.get(k) {
                Some(Value::String(w)) if w != v => {
                    return Err(bad(format!(
                        "the context `{k}` has two values in one batch"
                    )));
                }
                _ => {
                    state.insert(k.clone(), Value::String(v.clone()));
                }
            }
        }
        let body = match &q.shape {
            Shape::Choice(os) => {
                let mut c: Map<String, Value> = os
                    .iter()
                    .map(|(o, d)| (o.clone(), Value::String(d.clone())))
                    .collect();
                if q.unsure_allowed {
                    c.insert("unsure".into(), json!("none of these fits"));
                }
                json!({"type": "choice", "instructions": q.instructions, "criteria": c})
            }
            Shape::Noul { yes, no } => {
                json!({"type": "noul", "instructions": q.instructions, "criteria": {"true": yes, "false": no}})
            }
            Shape::Score(ls) => {
                json!({"type": "score", "instructions": q.instructions, "criteria": ls})
            }
        };
        if questions.insert(q.kind.clone(), body).is_some() {
            return Err(bad(format!(
                "two questions of kind `{}` in one batch",
                q.kind
            )));
        }
    }
    Ok(json!({"state": state, "questions": questions}))
}

/// Jev's reply, one answer a question in order, each validated. A reply
/// that is not JSON, lacks a question, or answers one outside its shape
/// makes the whole batch unsure.
pub fn jev_answers(qs: &[Question], raw: &[u8]) -> Vec<Weighed> {
    let all_unsure = || qs.iter().map(|_| Weighed::unsure()).collect();
    let Ok(v) = serde_json::from_slice::<Value>(raw) else {
        return all_unsure();
    };
    let mut out = Vec::new();
    for q in qs {
        match v["answers"].get(&q.kind).and_then(|a| answer(q, a)) {
            Some(w) => out.push(w),
            None => return all_unsure(),
        }
    }
    out
}

fn confidence(a: &Value) -> Confidence {
    a["confidence"]
        .as_f64()
        .map_or(Confidence::Unknown, Confidence::Known)
}

/// One answer, or None when it does not fit the question's shape.
fn answer(q: &Question, a: &Value) -> Option<Weighed> {
    let t = a["type"].as_str()?;
    let ans = match (&q.shape, t) {
        (Shape::Choice(os), "choice") => match a["choice"].as_str()? {
            "unsure" => Answer::Unsure,
            c if os.iter().any(|(o, _)| o == c) => Answer::Chosen(c.into(), confidence(a)),
            _ => return None,
        },
        (Shape::Noul { .. }, "noul") => {
            let p = a["noul"].as_f64()?;
            if p >= 0.5 {
                Answer::Yes(Confidence::Known(p))
            } else {
                Answer::No(Confidence::Known(1.0 - p))
            }
        }
        (Shape::Score(ls), "score") => {
            let ps = a["probabilities"].as_object()?;
            let (n, _) = ps
                .iter()
                .filter_map(|(k, v)| Some((k.parse::<u8>().ok()?, v.as_f64()?)))
                .max_by(|x, y| x.1.total_cmp(&y.1))?;
            if usize::from(n) >= ls.len() {
                return None;
            }
            Answer::Level(
                n,
                a["score"].as_f64().unwrap_or(f64::from(n)),
                confidence(a),
            )
        }
        _ => return None,
    };
    let mut distribution: Vec<(String, f64)> = match (&q.shape, a["probabilities"].as_object()) {
        (Shape::Choice(_), Some(ps)) => ps
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_f64()?)))
            .collect(),
        _ => Vec::new(),
    };
    distribution.sort_by(|x, y| y.1.total_cmp(&x.1));
    Some(Weighed {
        answer: ans,
        distribution,
    })
}

/// A question's identity for a recording: sixteen hex digits of FNV-1a 64
/// over its Jev body alone.
pub fn question_key(q: &Question) -> String {
    let body = jev_body(std::slice::from_ref(q)).unwrap_or(Value::Null);
    fnv64_hex(&serde_json::to_vec(&body).unwrap_or_default())
}

#[derive(Clone)]
pub struct Jev {
    model: String,
    key: String,
    url: String,
    ledger: Option<Arc<dyn Ledger>>,
    product: String,
    backoff: Duration,
}

impl Jev {
    /// Jev with its key from `[decide] key_env` (default `JEV_API_KEY`).
    pub fn from_env(
        cfg: &DecideConfig,
        env: &dyn Env,
        ledger: Option<Arc<dyn Ledger>>,
    ) -> Result<Jev> {
        let var = cfg.key_env.clone().unwrap_or_else(|| "JEV_API_KEY".into());
        let key = env.var(&var).ok_or(Error::NoKey {
            provider: "jev".into(),
            var,
        })?;
        Ok(Jev {
            model: cfg.model.clone().unwrap_or_else(|| MODEL.into()),
            key,
            url: URL.into(),
            ledger,
            product: String::new(),
            backoff: Duration::from_secs(1),
        })
    }

    /// The same Jev, recording its calls as PRODUCT's.
    pub fn for_product(&self, product: &str) -> Jev {
        Jev {
            product: product.into(),
            ..self.clone()
        }
    }

    #[doc(hidden)]
    pub fn with_url(mut self, url: &str) -> Jev {
        self.url = url.trim_end_matches('/').into();
        self
    }

    #[doc(hidden)]
    pub fn with_backoff(mut self, backoff: Duration) -> Jev {
        self.backoff = backoff;
        self
    }

    /// The raw reply's `answers` object for QS (for recording).
    pub fn ask_raw(&self, qs: &[Question], cancel: &Cancel) -> Result<Value> {
        let mut body = jev_body(qs)?;
        body["model"] = json!(self.model);
        let post = Post {
            provider: "jev",
            url: format!("{}/v1/systemone", self.url),
            headers: vec![("authorization".into(), format!("Bearer {}", self.key))],
            timeout: Duration::from_secs(30),
            backoff: self.backoff,
        };
        let started = Instant::now();
        let result = post_json(&post, &body, cancel);
        if let Some(l) = &self.ledger {
            let (input, cost) = match &result {
                Ok((v, _)) => (
                    v["usage"]["input_tokens"]
                        .as_u64()
                        .unwrap_or(body.to_string().len() as u64 / 4),
                    v["usage"]["cost_usd"].as_f64(),
                ),
                Err(_) => (0, None),
            };
            l.append(&Errand {
                at: chrono::Utc::now(),
                product: self.product.clone(),
                verb: "decide".into(),
                model: self.model.clone(),
                provider: "jev".into(),
                cost_usd: cost,
                input_tokens: input,
                output_tokens: 0,
                seconds: started.elapsed().as_secs_f64(),
                ok: result.is_ok(),
                note: result.as_ref().err().map(ToString::to_string),
            })?;
        }
        result.map(|(v, _)| v)
    }
}

impl Decider for Jev {
    fn ask(&self, qs: &[Question], cancel: &Cancel) -> Result<Vec<Weighed>> {
        if qs.is_empty() {
            return Ok(Vec::new());
        }
        match self.ask_raw(qs, cancel) {
            Ok(v) => Ok(jev_answers(qs, v.to_string().as_bytes())),
            Err(Error::Cancelled) => Err(Error::Cancelled),
            Err(Error::Core(e)) => Err(Error::Core(e)),
            Err(_) => Ok(qs.iter().map(|_| Weighed::unsure()).collect()),
        }
    }
}
