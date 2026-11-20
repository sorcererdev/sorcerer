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

use crate::adapters::http::{JsonReply, Post, post_json};
use crate::domain::model::{
    Completion, Content, ModelId, Reply, Role, StopReason, ToolCall, Usage, price,
};
use crate::ports::provider::Provider;
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
    /// From `[model.providers.<name>]`. The `url` is required; the key is optional (a
    /// local server such as Ollama needs none) and is read from the variable `api_key_env`
    /// names. A price may be keyed by an alias or by a model id.
    ///
    /// Example: `url = "http://localhost:11434/v1"` with `aliases.haiku = "qwen3:8b"` sends
    /// requests for `haiku` to Ollama's `qwen3:8b`.
    pub fn from_config(
        name: &str,
        cfg: &ProviderConfig,
        env: &dyn Env,
    ) -> Result<OpenAiCompatible> {
        let url = cfg.url.clone().ok_or_else(|| Error::Setup {
            why: format!("[model.providers.{name}] needs a url, such as http://localhost:11434/v1"),
        })?;
        let prices = cfg
            .prices
            .iter()
            .map(|(alias_or_id, price)| {
                let id = cfg.aliases.get(alias_or_id).unwrap_or(alias_or_id);
                (id.clone(), *price)
            })
            .collect();
        Ok(OpenAiCompatible {
            name: name.into(),
            url: url.trim_end_matches('/').into(),
            key: cfg.api_key_env.as_deref().and_then(|var| env.var(var)),
            aliases: cfg.aliases.clone(),
            prices,
            backoff: Duration::from_secs(1),
        })
    }

    /// The wait before the first retry when the server is busy (tests set zero).
    pub fn with_backoff(mut self, backoff: Duration) -> OpenAiCompatible {
        self.backoff = backoff;
        self
    }
}

/// The chat completions request: the model, the conversation, the token limit, and the
/// tools in OpenAI's function shape when there are any.
fn body(req: &Completion) -> Value {
    let mut body =
        json!({"model": req.model.0, "messages": messages(req), "max_tokens": req.max_tokens});
    if !req.tools.is_empty() {
        body["tools"] = req.tools.iter().map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}})).collect();
    }
    body
}

/// The conversation in OpenAI's shape. The system prompt is the first message. OpenAI
/// wants each tool result as its own `tool` message, so a user turn becomes its tool
/// results followed by its text.
///
/// Example: a user turn holding two tool results and the text "go on" becomes three
/// messages: `tool`, `tool`, then `user` with "go on".
fn messages(req: &Completion) -> Vec<Value> {
    std::iter::once(json!({"role": "system", "content": req.system}))
        .chain(req.messages.iter().flat_map(|m| match m.role {
            Role::Assistant => vec![assistant_message(&m.content)],
            Role::User => tool_results_then_text(&m.content),
        }))
        .collect()
}

/// The model's turn: its text and the tools it asked to run, each left out when empty.
fn assistant_message(content: &[Content]) -> Value {
    let text = text_of(content);
    let calls: Vec<Value> = content
        .iter()
        .filter_map(|c| match c {
            Content::ToolCall(t) => Some(json!({"id": t.id, "type": "function", "function": {"name": t.name, "arguments": t.input.to_string()}})),
            _ => None,
        })
        .collect();
    let mut message = json!({"role": "assistant"});
    if !text.is_empty() {
        message["content"] = json!(text);
    }
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    message
}

fn tool_results_then_text(content: &[Content]) -> Vec<Value> {
    let tool_results = content.iter().filter_map(|c| match c {
        Content::ToolResult { id, text } => {
            Some(json!({"role": "tool", "tool_call_id": id, "content": text}))
        }
        _ => None,
    });
    let text = text_of(content);
    let user_text = (!text.is_empty()).then(|| json!({"role": "user", "content": text}));
    tool_results.chain(user_text).collect()
}

/// The turn's text pieces joined by a blank line.
fn text_of(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            Content::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

impl Provider for OpenAiCompatible {
    fn name(&self) -> &str {
        &self.name
    }

    /// The table's alias first; else a name that looks like the server's own model name
    /// (it has a `/` or a `:`, such as `qwen3:8b`) as it is.
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

    /// Sends the conversation to `/chat/completions` and reads the first choice: its text,
    /// its tool calls, the tokens used and their price, and why the model stopped.
    fn complete(&self, req: Completion, cancel: &Cancel) -> Result<Reply> {
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
        let JsonReply { json, seconds } = post_json(&post, &body(&req), cancel)?;
        let choice = &json["choices"][0];
        if choice.is_null() {
            return Err(Error::Reply {
                provider: self.name.clone(),
                why: "no choices in the reply".into(),
            });
        }
        let usage = Usage {
            input_tokens: json["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: json["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        };
        Ok(Reply {
            text: choice["message"]["content"].as_str().unwrap_or_default().into(),
            tool_calls: tool_calls_of(&choice["message"]),
            usage,
            cost_usd: price(&self.prices, &req.model.0, &usage),
            stop: stop_reason(choice["finish_reason"].as_str().unwrap_or_default()),
            seconds,
        })
    }

    fn supports_tools(&self) -> bool {
        true
    }
}

/// The tools the model asked to run. OpenAI sends each call's input as a JSON string; one
/// that does not parse is kept as that string so the tool can say what is wrong.
fn tool_calls_of(message: &Value) -> Vec<ToolCall> {
    let Some(calls) = message["tool_calls"].as_array() else {
        return Vec::new();
    };
    calls
        .iter()
        .map(|call| {
            let arguments = call["function"]["arguments"].as_str().unwrap_or("{}");
            ToolCall {
                id: call["id"].as_str().unwrap_or_default().into(),
                name: call["function"]["name"].as_str().unwrap_or_default().into(),
                input: serde_json::from_str(arguments)
                    .unwrap_or_else(|_| Value::String(arguments.into())),
            }
        })
        .collect()
}

fn stop_reason(finish_reason: &str) -> StopReason {
    match finish_reason {
        "stop" => StopReason::End,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        other => StopReason::Other(other.into()),
    }
}
