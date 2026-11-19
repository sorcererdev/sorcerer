//! An OpenAI-compatible server: `POST {url}/chat/completions`, tools in
//! the OpenAI function shape. Ollama, LM Studio, vLLM, OpenAI and most
//! others speak it. There are no shipped aliases: the table's `aliases`
//! say which model `sonnet` or `haiku` means on this server.

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

pub struct OpenAiCompatible {
    name: String,
    url: String,
    key: Option<String>,
    aliases: BTreeMap<String, String>,
    prices: BTreeMap<String, [f64; 2]>,
    backoff: Duration,
}

impl OpenAiCompatible {
    /// From `[model.providers.<name>]`: `url` required, the key optional
    /// (from `api_key_env`).
    pub fn from_config(
        name: &str,
        cfg: &ProviderConfig,
        env: &dyn Env,
    ) -> Result<OpenAiCompatible> {
        let url = cfg.url.clone().ok_or_else(|| Error::Setup {
            why: format!("[model.providers.{name}] needs a url, such as http://localhost:11434/v1"),
        })?;
        let key = cfg.api_key_env.as_deref().and_then(|v| env.var(v));
        let prices = cfg
            .prices
            .iter()
            .map(|(k, p)| (cfg.aliases.get(k).cloned().unwrap_or_else(|| k.clone()), *p))
            .collect();
        Ok(OpenAiCompatible {
            name: name.into(),
            url: url.trim_end_matches('/').into(),
            key,
            aliases: cfg.aliases.clone(),
            prices,
            backoff: Duration::from_secs(1),
        })
    }

    pub fn with_backoff(mut self, backoff: Duration) -> OpenAiCompatible {
        self.backoff = backoff;
        self
    }
}

fn messages(req: &Completion) -> Vec<Value> {
    let mut out = vec![json!({"role": "system", "content": req.system})];
    for m in &req.messages {
        let text: String = m
            .content
            .iter()
            .filter_map(|c| {
                if let Content::Text(t) = c {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let calls: Vec<Value> = m
            .content
            .iter()
            .filter_map(|c| match c {
                Content::ToolCall(t) => Some(json!({"id": t.id, "type": "function", "function": {"name": t.name, "arguments": t.input.to_string()}})),
                _ => None,
            })
            .collect();
        let results: Vec<Value> = m
            .content
            .iter()
            .filter_map(|c| match c {
                Content::ToolResult { id, text } => {
                    Some(json!({"role": "tool", "tool_call_id": id, "content": text}))
                }
                _ => None,
            })
            .collect();
        match m.role {
            Role::Assistant => {
                let mut a = json!({"role": "assistant"});
                if !text.is_empty() {
                    a["content"] = json!(text);
                }
                if !calls.is_empty() {
                    a["tool_calls"] = json!(calls);
                }
                out.push(a);
            }
            Role::User => {
                out.extend(results);
                if !text.is_empty() {
                    out.push(json!({"role": "user", "content": text}));
                }
            }
        }
    }
    out
}

impl Provider for OpenAiCompatible {
    fn name(&self) -> &str {
        &self.name
    }

    fn resolve(&self, alias: &str) -> Result<ModelId> {
        if let Some(m) = self.aliases.get(alias) {
            return Ok(ModelId(m.clone()));
        }
        if alias.contains('/') || alias.contains(':') {
            return Ok(ModelId(alias.into()));
        }
        Err(Error::UnknownAlias {
            provider: self.name.clone(),
            alias: alias.into(),
            hint: format!(
                "set [model.providers.{}] aliases.{alias} = \"<model>\"",
                self.name
            ),
        })
    }

    fn complete(&self, req: Completion, cancel: &Cancel) -> Result<Reply> {
        let mut body =
            json!({"model": req.model.0, "messages": messages(&req), "max_tokens": req.max_tokens});
        if !req.tools.is_empty() {
            body["tools"] = req.tools.iter().map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}})).collect();
        }
        let post = Post {
            provider: &self.name,
            url: format!("{}/chat/completions", self.url),
            headers: self
                .key
                .iter()
                .map(|k| ("authorization".to_string(), format!("Bearer {k}")))
                .collect(),
            timeout: req.timeout,
            backoff: self.backoff,
        };
        let (v, seconds) = post_json(&post, &body, cancel)?;
        let choice = &v["choices"][0];
        if choice.is_null() {
            return Err(Error::Reply {
                provider: self.name.clone(),
                why: "no choices in the reply".into(),
            });
        }
        let msg = &choice["message"];
        let tool_calls = msg["tool_calls"]
            .as_array()
            .map(|cs| {
                cs.iter()
                    .map(|c| {
                        let args = c["function"]["arguments"].as_str().unwrap_or("{}");
                        ToolCall {
                            id: c["id"].as_str().unwrap_or_default().into(),
                            name: c["function"]["name"].as_str().unwrap_or_default().into(),
                            input: serde_json::from_str(args)
                                .unwrap_or_else(|_| Value::String(args.into())),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let usage = Usage {
            input_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        };
        let stop = match choice["finish_reason"].as_str().unwrap_or_default() {
            "stop" => StopReason::End,
            "tool_calls" => StopReason::ToolUse,
            "length" => StopReason::MaxTokens,
            other => StopReason::Other(other.into()),
        };
        Ok(Reply {
            text: msg["content"].as_str().unwrap_or_default().into(),
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
