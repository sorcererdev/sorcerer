//! Anthropic's Messages API: `POST {url}/v1/messages`, the key from
//! `ANTHROPIC_API_KEY` (or the table's `api_key_env`), tool use native.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};
use sorcerer_core::Cancel;
use sorcerer_core::domain::config::ProviderConfig;
use sorcerer_core::ports::env::Env;

use super::{
    Completion, Content, ModelId, Post, Provider, Reply, Role, StopReason, ToolCall, Usage,
    post_json, price,
};
use crate::{Error, Result};

/// Model ids by alias. `strong` and `cheap` are what products ask for when
/// they mean "the best" and "the cheapest that will do".
const ALIASES: &[(&str, &str)] = &[
    ("sonnet", "claude-sonnet-5-5"),
    ("haiku", "claude-haiku-5-5"),
    ("opus", "claude-opus-5-5"),
    ("strong", "claude-sonnet-5-5"),
    ("cheap", "claude-haiku-5-5"),
];

/// Dollars per million tokens, `[input, output]`, from Anthropic's pricing
/// page on 2026-11-17 (Haiku's is for prompts up to 100,000 tokens).
const PRICES: &[(&str, [f64; 2])] = &[
    ("claude-sonnet-5-5", [2.0, 10.0]),
    ("claude-haiku-5-5", [0.10, 0.50]),
    ("claude-opus-5-5", [4.0, 20.0]),
];

pub struct Anthropic {
    url: String,
    key: String,
    aliases: BTreeMap<String, String>,
    prices: BTreeMap<String, [f64; 2]>,
    backoff: Duration,
}

impl Anthropic {
    pub fn from_config(cfg: &ProviderConfig, env: &dyn Env) -> Result<Anthropic> {
        let var = cfg
            .api_key_env
            .clone()
            .unwrap_or_else(|| "ANTHROPIC_API_KEY".into());
        let key = env.var(&var).ok_or(Error::NoKey {
            provider: "anthropic".into(),
            var,
        })?;
        let mut aliases: BTreeMap<String, String> = ALIASES
            .iter()
            .map(|(a, m)| (a.to_string(), m.to_string()))
            .collect();
        aliases.extend(cfg.aliases.clone());
        let mut prices: BTreeMap<String, [f64; 2]> =
            PRICES.iter().map(|(m, p)| (m.to_string(), *p)).collect();
        for (k, p) in &cfg.prices {
            // a price may be keyed by alias or by id
            prices.insert(aliases.get(k).cloned().unwrap_or_else(|| k.clone()), *p);
        }
        let url = cfg
            .url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com".into());
        Ok(Anthropic {
            url: url.trim_end_matches('/').into(),
            key,
            aliases,
            prices,
            backoff: Duration::from_secs(1),
        })
    }

    /// The base wait before a retry (tests set zero).
    pub fn with_backoff(mut self, backoff: Duration) -> Anthropic {
        self.backoff = backoff;
        self
    }
}

fn block(c: &Content) -> Value {
    match c {
        Content::Text(t) => json!({"type": "text", "text": t}),
        Content::ToolCall(t) => {
            json!({"type": "tool_use", "id": t.id, "name": t.name, "input": t.input})
        }
        Content::ToolResult { id, text } => {
            json!({"type": "tool_result", "tool_use_id": id, "content": text})
        }
    }
}

pub(crate) fn body(req: &Completion) -> Value {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let role = if m.role == Role::User {
                "user"
            } else {
                "assistant"
            };
            json!({"role": role, "content": m.content.iter().map(block).collect::<Vec<_>>()})
        })
        .collect();
    let mut b = json!({"model": req.model.0, "system": req.system, "max_tokens": req.max_tokens, "messages": messages});
    if !req.tools.is_empty() {
        b["tools"] = req
            .tools
            .iter()
            .map(
                |t| json!({"name": t.name, "description": t.description, "input_schema": t.schema}),
            )
            .collect();
    }
    b
}

impl Provider for Anthropic {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn resolve(&self, alias: &str) -> Result<ModelId> {
        if alias.starts_with("claude-") {
            return Ok(ModelId(alias.into()));
        }
        self.aliases
            .get(alias)
            .map(|m| ModelId(m.clone()))
            .ok_or_else(|| Error::UnknownAlias {
                provider: "anthropic".into(),
                alias: alias.into(),
                hint: format!("set [model.providers.anthropic] aliases.{alias} = \"<model id>\""),
            })
    }

    fn complete(&self, req: Completion, cancel: &Cancel) -> Result<Reply> {
        let post = Post {
            provider: "anthropic",
            url: format!("{}/v1/messages", self.url),
            headers: vec![
                ("x-api-key".into(), self.key.clone()),
                ("anthropic-version".into(), "2023-06-01".into()),
            ],
            timeout: req.timeout,
            backoff: self.backoff,
        };
        let (v, seconds) = post_json(&post, &body(&req), cancel)?;
        let bad = |why: &str| Error::Reply {
            provider: "anthropic".into(),
            why: why.into(),
        };
        let content = v["content"]
            .as_array()
            .ok_or_else(|| bad("no content in the reply"))?;
        let text = content
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
        let tool_calls = content
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| ToolCall {
                id: b["id"].as_str().unwrap_or_default().into(),
                name: b["name"].as_str().unwrap_or_default().into(),
                input: b["input"].clone(),
            })
            .collect();
        let usage = Usage {
            input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
        };
        let stop = match v["stop_reason"].as_str().unwrap_or_default() {
            "end_turn" | "stop_sequence" => StopReason::End,
            "tool_use" => StopReason::ToolUse,
            "max_tokens" => StopReason::MaxTokens,
            other => StopReason::Other(other.into()),
        };
        Ok(Reply {
            text,
            tool_calls,
            usage,
            cost_usd: price(&self.prices, &req.model.0, &usage),
            stop,
            seconds,
        })
    }

    fn supports_tools(&self) -> bool {
        true
    }
}
