//! How a helper process (the MCP server, a hook) knows its way back to the server: three
//! environment variables the agent's process carries.

use sorcerer_core::ports::env::Env;

use crate::domain::loopback_message::{
    LoopbackReply, LoopbackRequest, PORT_VAR, RequestBody, TASK_VAR, TOKEN_VAR,
};

use super::loopback::ask_loopback;

/// The port, token and task a helper process was started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperWay {
    pub port: u16,
    pub token: String,
    pub task: String,
}

impl HelperWay {
    /// The way back from the environment; `None` when a variable is missing or the port is not
    /// a number (the program was started by hand, not by a task).
    pub fn from_env(env: &dyn Env) -> Option<HelperWay> {
        Some(HelperWay {
            port: env.var(PORT_VAR)?.parse().ok()?,
            token: env.var(TOKEN_VAR)?,
            task: env.var(TASK_VAR)?,
        })
    }

    /// Asks the server; a server that cannot be reached is an error reply saying so.
    pub fn ask(&self, body: RequestBody) -> LoopbackReply {
        let request = LoopbackRequest {
            token: self.token.clone(),
            task: self.task.clone(),
            body,
        };
        ask_loopback(self.port, &request).unwrap_or_else(|e| {
            LoopbackReply::error(format!("apprentice's server cannot be reached: {e}"))
        })
    }
}
