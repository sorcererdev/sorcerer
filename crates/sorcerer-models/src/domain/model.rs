//! A model call as data: which model, the conversation, the tools it may
//! use, the request and its reply, and what it cost. Products name models
//! by alias (`sonnet`, `haiku`); a provider turns the alias into the
//! vendor's model id, so no product names one.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The name a provider knows a model by: an alias such as `sonnet`, or the
/// vendor's own id such as `claude-sonnet-5-5`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelId(pub String);

/// Who says a message in a conversation: the user (sorcerer) or the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// One piece of a message: plain text, a tool the model asks to run, or
/// what that tool gave back (matched to the request by its `id`).
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(String),
    ToolCall(ToolCall),
    ToolResult { id: String, text: String },
}

/// One turn of a conversation: who speaks and what they say.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Content>,
}

impl Message {
    /// A user turn that is only text, the usual first message of a call.
    pub fn user(text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![Content::Text(text.into())],
        }
    }
}

/// A tool the model may ask to run, such as `read_file`, described so the
/// model knows when to use it and what to pass.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// The JSON schema of the tool's input.
    pub schema: serde_json::Value,
}

/// The model asking to run a tool: which tool, with what input, and an id
/// the answer must carry back.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// Everything one call to a model needs: which model, the instructions
/// (`system`), the conversation so far, the tools it may use, how long its
/// answer may be, and how long to wait for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub model: ModelId,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    pub timeout: Duration,
}

/// How many tokens a call read and wrote, which is what it is billed by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Why the model stopped writing: it finished, it wants a tool run, it hit
/// the `max_tokens` limit, or a reason the provider named that we do not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    End,
    ToolUse,
    MaxTokens,
    Other(String),
}

/// What a model call gave back: its text, the tools it asks to run, the
/// tokens and dollars it cost (no dollars when the provider has no price for
/// the model), why it stopped, and how many seconds it took.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Usage,
    pub cost_usd: Option<f64>,
    pub stop: StopReason,
    pub seconds: f64,
}

/// What a call cost in dollars, from a price list of dollars per million
/// tokens, `[input, output]`, keyed by alias or model id. A model missing
/// from the list has no price.
///
/// Example: with `sonnet = [3.0, 15.0]`, a call that read 1,000 tokens and
/// wrote 2,000 costs (1,000 × 3 + 2,000 × 15) / 1,000,000 = $0.033.
pub fn price(prices: &BTreeMap<String, [f64; 2]>, model: &str, usage: &Usage) -> Option<f64> {
    let [input_per_million, output_per_million] = prices.get(model)?;
    let input_dollars = usage.input_tokens as f64 * input_per_million / 1_000_000.0;
    let output_dollars = usage.output_tokens as f64 * output_per_million / 1_000_000.0;
    Some(input_dollars + output_dollars)
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
}
