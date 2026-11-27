//! What a task has cost so far, from the tokens its stream showed: each of the agent's messages
//! carries the tokens its call used, so a task the person stopped is charged what was seen,
//! not its whole cap.

use std::collections::BTreeMap;

use serde_json::Value;

/// The tokens one call of the agent used, as its message reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    /// Read from the prompt cache: a tenth of the input price.
    pub cache_read: u64,
    /// Written to the cache for five minutes: 1.25 times the input price.
    pub cache_write_5m: u64,
    /// Written to the cache for an hour: twice the input price.
    pub cache_write_1h: u64,
}

/// The usage of an assistant message's `usage` object; `None` when it has none. A cache write
/// with no split by lifetime counts as a five-minute one.
///
/// Example: `{"input_tokens": 8, "output_tokens": 141, "cache_read_input_tokens": 5024,
/// "cache_creation_input_tokens": 1896}` is 8 in, 141 out, 5,024 read, 1,896 written for 5m.
pub fn usage_of(usage: &Value) -> Option<TokenUsage> {
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    usage.as_object()?;
    let written = count("cache_creation_input_tokens");
    let split = &usage["cache_creation"];
    let (write_5m, write_1h) = if split.is_object() {
        let five = split["ephemeral_5m_input_tokens"].as_u64().unwrap_or(0);
        let hour = split["ephemeral_1h_input_tokens"].as_u64().unwrap_or(0);
        (five, hour)
    } else {
        (written, 0)
    };
    Some(TokenUsage {
        input: count("input_tokens"),
        output: count("output_tokens"),
        cache_read: count("cache_read_input_tokens"),
        cache_write_5m: write_5m,
        cache_write_1h: write_1h,
    })
}

/// The calls a task's stream showed, by message id: a message the stream repeats (one line per
/// block) counts once, at its last usage.
#[derive(Debug, Clone, Default)]
pub struct UsageSeen {
    calls: BTreeMap<String, (String, TokenUsage)>,
}

impl UsageSeen {
    /// One message's usage, with its model.
    pub fn saw(&mut self, message_id: &str, model: &str, usage: TokenUsage) {
        self.calls
            .insert(message_id.to_string(), (model.to_string(), usage));
    }

    /// The dollars the calls seen cost, at `price_of` a model (`[input, output]` per million
    /// tokens); `None` when none was seen, or one's model has no price.
    ///
    /// Example: one sonnet call ($2 in, $10 out) of 1,643 out, 79,554 read and 8,739 written for
    /// an hour is about $0.067.
    pub fn cost(&self, price_of: impl Fn(&str) -> Option<[f64; 2]>) -> Option<f64> {
        if self.calls.is_empty() {
            return None;
        }
        self.calls.values().try_fold(0.0, |sum, (model, usage)| {
            let [input, output] = price_of(model)?;
            Some(sum + dollars(usage, input, output))
        })
    }
}

fn dollars(usage: &TokenUsage, input: f64, output: f64) -> f64 {
    let input_tokens = usage.input as f64
        + usage.cache_read as f64 * 0.1
        + usage.cache_write_5m as f64 * 1.25
        + usage.cache_write_1h as f64 * 2.0;
    (input_tokens * input + usage.output as f64 * output) / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_repeated_message_counts_once_and_cache_is_priced_by_kind() {
        let usage = usage_of(&json!({
            "input_tokens": 8, "output_tokens": 1643, "cache_read_input_tokens": 79554,
            "cache_creation_input_tokens": 8739,
            "cache_creation": { "ephemeral_1h_input_tokens": 8739, "ephemeral_5m_input_tokens": 0 },
        }))
        .unwrap();
        let mut seen = UsageSeen::default();
        seen.saw("msg_1", "claude-sonnet-5-5", usage);
        seen.saw("msg_1", "claude-sonnet-5-5", usage);
        let cost = seen.cost(|_| Some([2.0, 10.0])).unwrap();
        assert!((cost - 0.0674).abs() < 0.0005, "{cost}");
    }

    #[test]
    fn nothing_seen_or_an_unpriced_model_is_no_cost() {
        assert_eq!(UsageSeen::default().cost(|_| Some([2.0, 10.0])), None);
        let mut seen = UsageSeen::default();
        seen.saw("m", "someone-elses-model", TokenUsage::default());
        assert_eq!(seen.cost(|_| None), None);
    }
}
