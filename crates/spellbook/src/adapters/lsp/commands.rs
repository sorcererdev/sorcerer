//! The four commands the editor's pickers run, and the capabilities that announce them.
//!
//! - `spellbook.generators []`
//! - `spellbook.choices [generator, inputs, input]`
//! - `spellbook.plan [generator, inputs]`
//! - `spellbook.apply [plan id]`
//!
//! A reply is JSON the plugin reads; a refusal the person must hear is `{ "stop": "<sentence>" }`
//! in the reply. An error answers only a malformed or unknown command.

use lsp_server::{ErrorCode, ResponseError};
use lsp_types::{ExecuteCommandOptions, ExecuteCommandParams, ServerCapabilities};
use serde_json::Value;

use super::Server;

pub(super) const GENERATORS: &str = "spellbook.generators";
pub(super) const CHOICES: &str = "spellbook.choices";
pub(super) const PLAN: &str = "spellbook.plan";
pub(super) const APPLY: &str = "spellbook.apply";

/// The four commands, and no code actions.
pub(super) fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: [GENERATORS, CHOICES, PLAN, APPLY]
                .map(String::from)
                .to_vec(),
            ..ExecuteCommandOptions::default()
        }),
        ..ServerCapabilities::default()
    }
}

impl Server {
    /// Runs one of the commands.
    pub(super) fn execute(&self, params: Value) -> Result<Value, ResponseError> {
        let params: ExecuteCommandParams = serde_json::from_value(params).map_err(invalid)?;
        Err(invalid(format!("no command {}", params.command)))
    }
}

pub(super) fn invalid(why: impl ToString) -> ResponseError {
    ResponseError {
        code: ErrorCode::InvalidParams as i32,
        message: why.to_string(),
        data: None,
    }
}
