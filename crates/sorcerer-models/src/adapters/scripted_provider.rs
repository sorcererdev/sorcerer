//! A provider for tests: its replies in order, every request recorded.

use std::collections::VecDeque;
use std::sync::Mutex;

use sorcerer_core::Cancel;

use crate::domain::model::{Completion, ModelId, Reply};
use crate::ports::provider::Provider;
use crate::{Error, Result};

/// Replies in order; every request recorded.
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
#[allow(clippy::too_many_lines)]
mod tests {
    use super::*;
    use crate::domain::model::{Message, StopReason, Usage};

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
