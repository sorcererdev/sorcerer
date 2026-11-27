//! How a helper process (the MCP server, a hook) knows its way back to the server: three
//! environment variables the agent's process carries.

use std::io::ErrorKind;

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

    /// Asks the server. A port that refuses the connection means the server has ended (the
    /// editor closed, or its server was killed) and is [`ServerAnswer::Gone`]; any other failure
    /// to reach it is an error reply saying so.
    ///
    /// Example: after `kill -9` of `lackey lsp`, nothing listens on its port, so every
    /// helper's next request is `Gone` at once, and the helper ends the agent's work.
    pub fn ask(&self, body: RequestBody) -> ServerAnswer {
        let request = LoopbackRequest {
            token: self.token.clone(),
            task: self.task.clone(),
            body,
        };
        match ask_loopback(self.port, &request) {
            Ok(reply) => ServerAnswer::Reply(reply),
            Err(e) if e.kind() == ErrorKind::ConnectionRefused => ServerAnswer::Gone,
            Err(e) => ServerAnswer::Reply(LoopbackReply::error(format!(
                "lackey's server cannot be reached: {e}"
            ))),
        }
    }
}

/// What a helper's request came back with.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerAnswer {
    Reply(LoopbackReply),
    /// Nothing listens on the port any more: the server that runs the task has ended, so the
    /// task is over and the agent should stop.
    Gone,
}

/// What the agent is told when the server has ended: in a tool's result, and as the reason the
/// hook stops it.
pub const SERVER_GONE: &str = "lackey's server has ended (the editor closed or its server stopped): the task is over; stop now and do nothing more";
