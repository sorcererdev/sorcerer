//! The provider seam: a way to run a model. A provider turns a request
//! into a reply and knows what it cost.

use sorcerer_core::Cancel;

use crate::Result;
use crate::domain::model::{Completion, ModelId, Reply};

pub trait Provider: Send + Sync {
    /// The provider's name: `anthropic`, `claude`, `openai-compatible`, or
    /// the name a project gave it in its config.
    fn name(&self) -> &str;
    /// The model id this provider sends for an alias or an id. Example: on
    /// `anthropic`, `sonnet` becomes `claude-sonnet-5-5`; an id passes
    /// through.
    fn resolve(&self, alias: &str) -> Result<ModelId>;
    /// Runs one turn of a conversation: the reply's text and, when tools
    /// were offered, the tools the model asks to run.
    fn complete(&self, completion: Completion, cancel: &Cancel) -> Result<Reply>;
    /// Whether the model may be offered tools (a researcher needs them).
    fn supports_tools(&self) -> bool;
}
