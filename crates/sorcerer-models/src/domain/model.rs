//! A model call as data: model ids, messages, tool specs, a completion
//! request and its reply, what it cost, and the ledger row it makes.
//! Products name models by alias (`sonnet`, `haiku`); a provider resolves
//! them, so no product names a vendor's model id.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::errand::Errand;

use crate::Result;

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

/// The ledger row for one call to PROVIDER at AT, from its reply or its failure.
pub fn errand(
    product: &str,
    verb: &str,
    provider: &str,
    model: &ModelId,
    r: &Result<Reply>,
    at: DateTime<Utc>,
) -> Errand {
    let (usage, cost, seconds, ok, note) = match r {
        Ok(reply) => (reply.usage, reply.cost_usd, reply.seconds, true, None),
        Err(e) => (Usage::default(), None, 0.0, false, Some(e.to_string())),
    };
    Errand {
        at,
        product: product.into(),
        verb: verb.into(),
        model: model.0.clone(),
        provider: provider.into(),
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

#[cfg(test)]
#[allow(clippy::too_many_lines)]
mod tests {
    use super::*;
    use crate::Error;
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
        let at = "2026-11-18T12:00:00Z".parse().unwrap();
        let e = errand(
            "scry",
            "review",
            "scripted",
            &ModelId("m".into()),
            &Ok(reply("hi")),
            at,
        );
        assert_eq!(e.at, at);
        assert_eq!(
            (
                e.product.as_str(),
                e.verb.as_str(),
                e.provider.as_str(),
                e.model.as_str()
            ),
            ("scry", "review", "scripted", "m")
        );
        assert_eq!(
            (e.cost_usd, e.input_tokens, e.output_tokens, e.ok),
            (Some(0.5), 1, 2, true)
        );
    }

    #[test]
    fn an_errand_from_a_failure_is_not_ok_and_notes_why() {
        let e = errand(
            "scry",
            "review",
            "scripted",
            &ModelId("m".into()),
            &Err(Error::Timeout {
                provider: "scripted".into(),
                seconds: 3,
            }),
            chrono::Utc::now(),
        );
        assert!(!e.ok);
        assert!(e.note.unwrap().contains("timed out"));
    }
}
