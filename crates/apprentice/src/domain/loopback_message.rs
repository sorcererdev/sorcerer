//! What the agent's helper processes (the MCP server and the hooks, children of the agent, not
//! of the server) ask the server over its loopback port, and what it answers: one line of JSON
//! each way. Every request carries the server's token, so no other program on the machine is
//! answered.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One request to the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoopbackRequest {
    pub token: String,
    /// The task the asking process belongs to.
    pub task: String,
    pub body: RequestBody,
}

/// What is asked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RequestBody {
    /// The person's edits since the task last looked (a hook, after each tool call or at the
    /// end of the turn); `event` is the hook's.
    Edits { event: String },
    /// A medium tool's call.
    Tool { name: String, arguments: Value },
    /// May the agent run this tool with this input?
    Permission { tool: String, input: Value },
}

/// The server's answer: the text for the tool's result or the hook's context, and whether it
/// is an error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoopbackReply {
    pub text: String,
    pub is_error: bool,
}

impl LoopbackReply {
    pub fn text(text: impl Into<String>) -> LoopbackReply {
        LoopbackReply {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> LoopbackReply {
        LoopbackReply {
            text: text.into(),
            is_error: true,
        }
    }
}

/// The names of the environment variables that tell a helper process the way back.
pub const PORT_VAR: &str = "APPRENTICE_PORT";
pub const TOKEN_VAR: &str = "APPRENTICE_TOKEN";
pub const TASK_VAR: &str = "APPRENTICE_TASK";

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_request_reads_back_as_written() {
        let request = LoopbackRequest {
            token: "k".into(),
            task: "t1".into(),
            body: RequestBody::Tool {
                name: "notify".into(),
                arguments: json!({ "text": "hi" }),
            },
        };
        let line = serde_json::to_string(&request).unwrap();
        assert!(line.contains(r#""kind":"tool""#), "{line}");
        assert_eq!(
            serde_json::from_str::<LoopbackRequest>(&line).unwrap(),
            request
        );
    }
}
