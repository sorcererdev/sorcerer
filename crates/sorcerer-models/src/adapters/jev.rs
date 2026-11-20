//! Jev (typesafe.ai): `POST https://api.typesafe.ai/v1/systemone` with one
//! `state` map and a `questions` map, the model pinned. A reply
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

use crate::adapters::http::{Post, post_json};
use crate::domain::decide::{Answer, Confidence, Question, Shape, UnsureAnswer, Weighed};
use crate::ports::decider::Decider;
use crate::{Error, Result};

/// The Jev model sorcerer pins, unless `[decide] model` names another.
/// The only variable the Jev key is read from.
pub const KEY_VAR: &str = "JEV_API_KEY";
pub const MODEL: &str = "jev-1.14.0";
pub const URL: &str = "https://api.typesafe.ai";

/// The request for a batch of questions, without the model. Jev takes one `state` map that
/// every question in the batch shares, and the questions keyed by their kind.
///
/// Example: two questions whose context both name `code` must carry the same `code` text;
/// if they differ, or two questions share a kind, the batch is an error.
pub fn jev_body(qs: &[Question]) -> Result<Value> {
    let state = shared_state(qs)?;
    let questions = questions_by_kind(qs)?;
    Ok(json!({"state": state, "questions": questions}))
}

fn batch_error(why: String) -> Error {
    Error::Reply {
        provider: "jev".into(),
        why,
    }
}

fn shared_state(qs: &[Question]) -> Result<Map<String, Value>> {
    qs.iter()
        .flat_map(|q| &q.context)
        .try_fold(Map::new(), |mut state, (name, text)| {
            match state.get(name) {
                Some(earlier) if earlier != text.as_str() => Err(batch_error(format!(
                    "the context `{name}` has two values in one batch"
                ))),
                _ => {
                    state.insert(name.clone(), Value::String(text.clone()));
                    Ok(state)
                }
            }
        })
}

fn questions_by_kind(qs: &[Question]) -> Result<Map<String, Value>> {
    qs.iter().try_fold(Map::new(), |mut questions, q| {
        if questions.insert(q.kind.clone(), question_body(q)).is_some() {
            return Err(batch_error(format!(
                "two questions of kind `{}` in one batch",
                q.kind
            )));
        }
        Ok(questions)
    })
}

/// One question in Jev's shape. A choice that allows unsure gets an extra option,
/// `unsure`, meaning "none of these fits".
fn question_body(q: &Question) -> Value {
    match &q.shape {
        Shape::Choice(options) => {
            let unsure = (q.unsure == UnsureAnswer::Allowed)
                .then(|| ("unsure".to_string(), json!("none of these fits")));
            let criteria: Map<String, Value> = options
                .iter()
                .map(|(name, meaning)| (name.clone(), Value::String(meaning.clone())))
                .chain(unsure)
                .collect();
            json!({"type": "choice", "instructions": q.instructions, "criteria": criteria})
        }
        Shape::Noul { yes, no } => {
            json!({"type": "noul", "instructions": q.instructions, "criteria": {"true": yes, "false": no}})
        }
        Shape::Score(levels) => {
            json!({"type": "score", "instructions": q.instructions, "criteria": levels})
        }
    }
}

/// Jev's answers, one per question in the order asked. A decider never guesses: a reply
/// that is not JSON, leaves a question out, or answers one outside its shape (a choice
/// that is not an option, a level past the last) makes every answer in the batch unsure.
pub fn jev_answers(qs: &[Question], raw: &[u8]) -> Vec<Weighed> {
    serde_json::from_slice::<Value>(raw)
        .ok()
        .and_then(|reply| {
            qs.iter()
                .map(|q| reply["answers"].get(&q.kind).and_then(|a| answer(q, a)))
                .collect::<Option<Vec<_>>>()
        })
        .unwrap_or_else(|| qs.iter().map(|_| Weighed::unsure()).collect())
}

/// One answer, or None when it does not fit the question's shape. For a choice, the
/// probability Jev gave each option comes along, most likely first.
fn answer(q: &Question, a: &Value) -> Option<Weighed> {
    let answer = match (&q.shape, a["type"].as_str()?) {
        (Shape::Choice(options), "choice") => chosen_option(options, a)?,
        (Shape::Noul { .. }, "noul") => yes_or_no(a)?,
        (Shape::Score(levels), "score") => level(levels, a)?,
        _ => return None,
    };
    let distribution = match &q.shape {
        Shape::Choice(_) => options_most_likely_first(a),
        _ => Vec::new(),
    };
    Some(Weighed {
        answer,
        distribution,
    })
}

/// `unsure`, or one of the question's options; any other name does not fit.
fn chosen_option(options: &[(String, String)], a: &Value) -> Option<Answer> {
    match a["choice"].as_str()? {
        "unsure" => Some(Answer::Unsure),
        name if options.iter().any(|(option, _)| option == name) => {
            Some(Answer::Chosen(name.into(), confidence(a)))
        }
        _ => None,
    }
}

/// Jev gives the probability of yes: 0.5 or more is yes with that confidence, below it
/// is no with the rest (0.2 → no, confidence 0.8).
fn yes_or_no(a: &Value) -> Option<Answer> {
    let yes = a["noul"].as_f64()?;
    Some(if yes >= 0.5 {
        Answer::Yes(Confidence::Known(yes))
    } else {
        Answer::No(Confidence::Known(1.0 - yes))
    })
}

/// The most likely level, which must be one of the question's levels, with Jev's score
/// on the scale (the level itself when Jev gives none).
fn level(levels: &[String], a: &Value) -> Option<Answer> {
    let (most_likely, _) = a["probabilities"]
        .as_object()?
        .iter()
        .filter_map(|(level, p)| Some((level.parse::<u8>().ok()?, p.as_f64()?)))
        .max_by(|x, y| x.1.total_cmp(&y.1))?;
    if usize::from(most_likely) >= levels.len() {
        return None;
    }
    let score = a["score"].as_f64().unwrap_or(f64::from(most_likely));
    Some(Answer::Level(most_likely, score, confidence(a)))
}

fn options_most_likely_first(a: &Value) -> Vec<(String, f64)> {
    let mut options: Vec<(String, f64)> = a["probabilities"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, p)| Some((name.clone(), p.as_f64()?)))
        .collect();
    options.sort_by(|x, y| y.1.total_cmp(&x.1));
    options
}

fn confidence(a: &Value) -> Confidence {
    a["confidence"]
        .as_f64()
        .map_or(Confidence::Unknown, Confidence::Known)
}

/// A question's identity in a recording: sixteen hex digits of an FNV-1a 64 hash of its
/// Jev request alone, so the same question finds its recorded answer in any batch.
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
    /// Jev with its key read from `JEV_API_KEY` only. With a ledger, every call is recorded there with its cost.
    pub fn from_env(
        cfg: &DecideConfig,
        env: &dyn Env,
        ledger: Option<Arc<dyn Ledger>>,
    ) -> Result<Jev> {
        let key = env.var(KEY_VAR).ok_or(Error::NoKey {
            provider: "jev".into(),
            var: KEY_VAR.into(),
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

    /// The same Jev, recording its calls in the ledger under the given product (`scry`).
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

    /// Asks Jev the batch and returns its whole reply unread, so it can be recorded.
    /// The model is pinned in the request, and the call (its tokens, cost, time and
    /// whether it failed) goes into the ledger before the result comes back.
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
        let result = post_json(&post, &body, cancel).map(|reply| reply.json);
        if let Some(ledger) = &self.ledger {
            ledger.append(&self.errand(&body, &result, started))?;
        }
        result
    }

    /// The ledger line for one call. When Jev does not say how many tokens it read, they
    /// are estimated at one per four characters of the request.
    fn errand(&self, body: &Value, result: &Result<Value>, started: Instant) -> Errand {
        let (input_tokens, cost_usd) = match result {
            Ok(reply) => (
                reply["usage"]["input_tokens"]
                    .as_u64()
                    .unwrap_or(body.to_string().len() as u64 / 4),
                reply["usage"]["cost_usd"].as_f64(),
            ),
            Err(_) => (0, None),
        };
        Errand {
            at: chrono::Utc::now(),
            product: self.product.clone(),
            verb: "decide".into(),
            model: self.model.clone(),
            provider: "jev".into(),
            cost_usd,
            input_tokens,
            output_tokens: 0,
            seconds: started.elapsed().as_secs_f64(),
            ok: result.is_ok(),
            note: result.as_ref().err().map(ToString::to_string),
        }
    }
}

impl Decider for Jev {
    /// An empty batch asks nothing. A call that fails makes every answer unsure, except a
    /// cancel or a ledger that cannot be written, which stop the caller.
    fn ask(&self, qs: &[Question], cancel: &Cancel) -> Result<Vec<Weighed>> {
        if qs.is_empty() {
            return Ok(Vec::new());
        }
        match self.ask_raw(qs, cancel) {
            Ok(reply) => Ok(jev_answers(qs, reply.to_string().as_bytes())),
            Err(Error::Cancelled) => Err(Error::Cancelled),
            Err(Error::Core(e)) => Err(Error::Core(e)),
            Err(_) => Ok(qs.iter().map(|_| Weighed::unsure()).collect()),
        }
    }
}
