//! The provider seam: a way to run a model. A provider turns a request
//! into a reply and knows what it cost. Products name models by alias
//! (`sonnet`, `haiku`); the provider resolves them, so no product names a
//! vendor's model id.

pub mod anthropic;
pub mod claude;
pub mod openai;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sorcerer_core::Cancel;
use sorcerer_core::ledger::Errand;

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(String),
    ToolCall(ToolCall),
    ToolResult { id: String, text: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Content>,
}

impl Message {
    pub fn user(text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![Content::Text(text.into())],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// The JSON schema of the tool's input.
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub model: ModelId,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    End,
    ToolUse,
    MaxTokens,
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Usage,
    pub cost_usd: Option<f64>,
    pub stop: StopReason,
    pub seconds: f64,
}

pub trait Provider: Send + Sync {
    /// `anthropic`, `claude`, `openai-compatible`, or a configured name.
    fn name(&self) -> &str;
    /// An alias (`sonnet`) or a model id to the id this provider sends.
    fn resolve(&self, alias: &str) -> Result<ModelId>;
    /// One turn: a reply with text and, when tools were offered, tool calls.
    fn complete(&self, req: Completion, cancel: &Cancel) -> Result<Reply>;
    fn supports_tools(&self) -> bool;
}

/// The ledger row for one call to PROVIDER, from its reply or its failure.
pub fn errand(
    product: &str,
    verb: &str,
    provider: &dyn Provider,
    model: &ModelId,
    r: &Result<Reply>,
) -> Errand {
    let (usage, cost, seconds, ok, note) = match r {
        Ok(reply) => (reply.usage, reply.cost_usd, reply.seconds, true, None),
        Err(e) => (Usage::default(), None, 0.0, false, Some(e.to_string())),
    };
    Errand {
        at: chrono::Utc::now(),
        product: product.into(),
        verb: verb.into(),
        model: model.0.clone(),
        provider: provider.name().into(),
        cost_usd: cost,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        seconds,
        ok,
        note,
    }
}

/// Dollars for USAGE of MODEL, from PRICES (`[input, output]` dollars per
/// million tokens, keyed by alias or model id).
pub fn price(prices: &BTreeMap<String, [f64; 2]>, model: &str, usage: &Usage) -> Option<f64> {
    let [i, o] = prices.get(model)?;
    Some((usage.input_tokens as f64 * i + usage.output_tokens as f64 * o) / 1_000_000.0)
}

/// How a JSON POST is sent: the provider's name (for errors), the URL,
/// headers, the timeout, and the base wait before a retry (1 s in use,
/// zero in tests).
pub(crate) struct Post<'a> {
    pub provider: &'a str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub timeout: Duration,
    pub backoff: Duration,
}

/// POSTs BODY and returns the reply's JSON and the seconds it took. 429
/// and 529 are retried twice, after BACKOFF and twice BACKOFF. The request
/// runs on its own thread so CANCEL is honoured within a tenth of a second
/// (the abandoned request finishes or times out on its own).
pub(crate) fn post_json(
    post: &Post,
    body: &serde_json::Value,
    cancel: &Cancel,
) -> Result<(serde_json::Value, f64)> {
    let start = Instant::now();
    for attempt in 0..3u32 {
        if attempt > 0 {
            wait(post.backoff * attempt, cancel)?;
        }
        let (status, text) = send(post, body, cancel)?;
        if (status == 429 || status == 529) && attempt < 2 {
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(Error::Http {
                provider: post.provider.into(),
                status,
                body: text.chars().take(500).collect(),
            });
        }
        let json = serde_json::from_str(&text).map_err(|e| Error::Reply {
            provider: post.provider.into(),
            why: format!("not JSON: {e}"),
        })?;
        return Ok((json, start.elapsed().as_secs_f64()));
    }
    unreachable!("the third attempt returns")
}

fn wait(d: Duration, cancel: &Cancel) -> Result<()> {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(20).min(end - Instant::now()));
    }
    Ok(())
}

fn send(post: &Post, body: &serde_json::Value, cancel: &Cancel) -> Result<(u16, String)> {
    let (tx, rx) = mpsc::channel();
    let client = reqwest::blocking::Client::builder()
        .timeout(post.timeout)
        .build()
        .map_err(|e| Error::Transport {
            provider: post.provider.into(),
            why: e.to_string(),
        })?;
    let mut rb = client
        .post(&post.url)
        .header("content-type", "application/json");
    for (k, v) in &post.headers {
        rb = rb.header(k, v);
    }
    let rb = rb.body(body.to_string());
    std::thread::spawn(move || {
        let r = rb.send().and_then(|resp| {
            let status = resp.status().as_u16();
            resp.text().map(|t| (status, t))
        });
        let _ = tx.send(r);
    });
    loop {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(r)) => return Ok(r),
            Ok(Err(e)) if e.is_timeout() => {
                return Err(Error::Timeout {
                    provider: post.provider.into(),
                    seconds: post.timeout.as_secs(),
                });
            }
            Ok(Err(e)) => {
                return Err(Error::Transport {
                    provider: post.provider.into(),
                    why: e.to_string(),
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Error::Transport {
                    provider: post.provider.into(),
                    why: "the request thread died".into(),
                });
            }
        }
    }
}

/// A provider for tests: its replies in order, every request recorded.
pub struct ScriptedProvider {
    replies: Mutex<VecDeque<Reply>>,
    pub seen: Mutex<Vec<Completion>>,
}

impl ScriptedProvider {
    pub fn new(replies: Vec<Reply>) -> ScriptedProvider {
        ScriptedProvider {
            replies: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
        }
    }
}

impl Provider for ScriptedProvider {
    fn name(&self) -> &str {
        "scripted"
    }

    fn resolve(&self, alias: &str) -> Result<ModelId> {
        Ok(ModelId(alias.into()))
    }

    fn complete(&self, req: Completion, _cancel: &Cancel) -> Result<Reply> {
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(req);
        self.replies
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
            .ok_or_else(|| Error::Reply {
                provider: "scripted".into(),
                why: "no more replies".into(),
            })
    }

    fn supports_tools(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn reply(text: &str) -> Reply {
        Reply {
            text: text.into(),
            tool_calls: vec![],
            usage: Usage {
                input_tokens: 1,
                output_tokens: 2,
            },
            cost_usd: Some(0.5),
            stop: StopReason::End,
            seconds: 1.0,
        }
    }

    #[test]
    fn price_of_a_known_model() {
        let prices = BTreeMap::from([("sonnet".to_string(), [3.0, 15.0])]);
        let p = price(
            &prices,
            "sonnet",
            &Usage {
                input_tokens: 1000,
                output_tokens: 2000,
            },
        )
        .unwrap();
        assert!((p - 0.033).abs() < 1e-12, "{p}");
    }

    #[test]
    fn price_of_an_unknown_model_is_none() {
        assert_eq!(price(&BTreeMap::new(), "x", &Usage::default()), None);
    }

    #[test]
    fn an_errand_from_a_reply_records_it() {
        let p = ScriptedProvider::new(vec![]);
        let e = errand("scry", "find", &p, &ModelId("m".into()), &Ok(reply("hi")));
        assert_eq!(
            (
                e.product.as_str(),
                e.verb.as_str(),
                e.provider.as_str(),
                e.model.as_str()
            ),
            ("scry", "find", "scripted", "m")
        );
        assert_eq!(
            (e.cost_usd, e.input_tokens, e.output_tokens, e.ok),
            (Some(0.5), 1, 2, true)
        );
    }

    #[test]
    fn an_errand_from_a_failure_is_not_ok_and_notes_why() {
        let p = ScriptedProvider::new(vec![]);
        let e = errand(
            "scry",
            "find",
            &p,
            &ModelId("m".into()),
            &Err(Error::Timeout {
                provider: "scripted".into(),
                seconds: 3,
            }),
        );
        assert!(!e.ok);
        assert!(e.note.unwrap().contains("timed out"));
    }

    #[test]
    fn scripted_provider_replies_in_order_and_records_requests() {
        let p = ScriptedProvider::new(vec![reply("one"), reply("two")]);
        let req = |s: &str| Completion {
            model: ModelId("m".into()),
            system: s.into(),
            messages: vec![Message::user("q")],
            tools: vec![],
            max_tokens: 10,
            timeout: std::time::Duration::from_secs(1),
        };
        let c = Cancel::default();
        assert_eq!(p.complete(req("a"), &c).unwrap().text, "one");
        assert_eq!(p.complete(req("b"), &c).unwrap().text, "two");
        assert!(p.complete(req("c"), &c).is_err());
        let seen = p.seen.lock().unwrap();
        assert_eq!(
            seen.iter().map(|r| r.system.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }
}
