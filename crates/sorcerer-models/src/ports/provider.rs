//! The provider seam: a way to run a model. A provider turns a request
//! into a reply and knows what it cost.

use sorcerer_core::Cancel;

use crate::Result;
use crate::domain::model::{Completion, ModelId, Reply};

pub trait Provider: Send + Sync {
    /// `anthropic`, `claude`, `openai-compatible`, or a configured name.
    fn name(&self) -> &str;
    /// An alias (`sonnet`) or a model id to the id this provider sends.
    fn resolve(&self, alias: &str) -> Result<ModelId>;
    /// One turn: a reply with text and, when tools were offered, tool calls.
    fn complete(&self, req: Completion, cancel: &Cancel) -> Result<Reply>;
    fn supports_tools(&self) -> bool;
}
