//! Anthropic's Messages API: `POST {url}/v1/messages`, the key from
//! `ANTHROPIC_API_KEY`, tool use native.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};
use sorcerer_core::Cancel;
use sorcerer_core::domain::config::ProviderConfig;
use sorcerer_core::ports::env::Env;

use crate::adapters::http::{JsonReply, Post, post_json};
use crate::domain::model::{
    Completion, Content, ModelId, Reply, Role, StopReason, ToolCall, Usage, price,
};
use crate::ports::provider::Provider;
use crate::{Error, Result};

/// Model ids by alias. `strong` and `cheap` are what products ask for when
/// they mean "the best" and "the cheapest that will do".
/// The only variable the Anthropic key is read from.
pub const KEY_VAR: &str = "ANTHROPIC_API_KEY";

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
    /// From `[model.providers.anthropic]`. The key is read from `ANTHROPIC_API_KEY` only, so
    /// a project's config cannot point scry at another secret; without it there is no provider. The
    /// table's aliases and prices are added to the shipped ones and win over them.
    ///
    /// Example: `prices.sonnet = [3.0, 15.0]` replaces the shipped price of
    /// `claude-sonnet-5-5`, because a price may be keyed by an alias or by a model id.
    pub fn from_config(cfg: &ProviderConfig, env: &dyn Env) -> Result<Anthropic> {
        let aliases = shipped_aliases_and(cfg);
        let prices = shipped_prices_and(cfg, &aliases);
        let url = cfg
            .url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com".into());
        Ok(Anthropic {
            url: url.trim_end_matches('/').into(),
            key: api_key(env)?,
            aliases,
            prices,
            backoff: Duration::from_secs(1),
        })
    }

    /// The wait before the first retry when Anthropic is busy (tests set zero).
    pub fn with_backoff(mut self, backoff: Duration) -> Anthropic {
        self.backoff = backoff;
        self
    }
}

/// The shipped price of a model, by its id or an alias, `[input, output]` dollars per million
/// tokens; `None` for a model the table does not know. For a cost seen outside this provider
/// (Claude Code's stream names the model of every call).
///
/// Example: `shipped_price("sonnet")` and `shipped_price("claude-sonnet-5-5")` are `[2.0, 10.0]`.
pub fn shipped_price(model: &str) -> Option<[f64; 2]> {
    let id = ALIASES
        .iter()
        .find(|(alias, _)| *alias == model)
        .map_or(model, |(_, id)| *id);
    PRICES
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, price)| *price)
}

fn api_key(env: &dyn Env) -> Result<String> {
    env.var(KEY_VAR).ok_or(Error::NoKey {
        provider: "anthropic".into(),
        var: KEY_VAR.into(),
    })
}

fn shipped_aliases_and(cfg: &ProviderConfig) -> BTreeMap<String, String> {
    ALIASES
        .iter()
        .map(|(alias, id)| (alias.to_string(), id.to_string()))
        .chain(cfg.aliases.clone())
        .collect()
}

fn shipped_prices_and(
    cfg: &ProviderConfig,
    aliases: &BTreeMap<String, String>,
) -> BTreeMap<String, [f64; 2]> {
    let from_table = cfg.prices.iter().map(|(alias_or_id, price)| {
        let id = aliases.get(alias_or_id).unwrap_or(alias_or_id);
        (id.clone(), *price)
    });
    PRICES
        .iter()
        .map(|(id, price)| (id.to_string(), *price))
        .chain(from_table)
        .collect()
}

/// The Messages API request: the model, the system prompt, the token limit, the
/// conversation, and the tools when there are any.
pub(crate) fn body(req: &Completion) -> Value {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| json!({"role": role_name(m.role), "content": m.content.iter().map(block).collect::<Vec<_>>()}))
        .collect();
    let mut body = json!({"model": req.model.0, "system": req.system, "max_tokens": req.max_tokens, "messages": messages});
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(
                |t| json!({"name": t.name, "description": t.description, "input_schema": t.schema}),
            )
            .collect();
    }
    body
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
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

impl Provider for Anthropic {
    fn name(&self) -> &str {
        "anthropic"
    }

    /// A full model id (`claude-...`) as it is; an alias through the shipped and the
    /// table's aliases (`sonnet` → `claude-sonnet-5-5`).
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

    /// Sends the conversation to the Messages API and reads the reply: the text blocks
    /// joined, the tool calls, the tokens used and their price, and why the model stopped.
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
        let JsonReply { json, seconds } = post_json(&post, &body(&req), cancel)?;
        let content = json["content"].as_array().ok_or_else(|| Error::Reply {
            provider: "anthropic".into(),
            why: "no content in the reply".into(),
        })?;
        let usage = Usage {
            input_tokens: json["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: json["usage"]["output_tokens"].as_u64().unwrap_or(0),
        };
        Ok(Reply {
            text: text_of(content),
            tool_calls: tool_calls_of(content),
            usage,
            cost_usd: price(&self.prices, &req.model.0, &usage),
            stop: stop_reason(json["stop_reason"].as_str().unwrap_or_default()),
            seconds,
        })
    }

    fn supports_tools(&self) -> bool {
        true
    }
}

fn text_of(content: &[Value]) -> String {
    content
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("")
}

fn tool_calls_of(content: &[Value]) -> Vec<ToolCall> {
    content
        .iter()
        .filter(|b| b["type"] == "tool_use")
        .map(|b| ToolCall {
            id: b["id"].as_str().unwrap_or_default().into(),
            name: b["name"].as_str().unwrap_or_default().into(),
            input: b["input"].clone(),
        })
        .collect()
}

fn stop_reason(anthropic_reason: &str) -> StopReason {
    match anthropic_reason {
        "end_turn" | "stop_sequence" => StopReason::End,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        other => StopReason::Other(other.into()),
    }
}
