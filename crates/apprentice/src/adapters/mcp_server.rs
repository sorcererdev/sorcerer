//! `apprentice mcp`: the medium tools as an MCP server over standard input and output, which
//! the agent starts. It is a thin translator: each `tools/call` becomes one loopback request to
//! the server that runs the task, and its answer one text block. When the server has ended
//! (its port refuses the connection), the call fails saying so and this server exits, so every
//! later tool call of the agent fails at once.
//!
//! The protocol is JSON-RPC 2.0, one message a line. Served: `initialize` (answered at once:
//! the agent waits at most 30 seconds for it), `tools/list`, `tools/call`, `ping`; notifications
//! are read and ignored; any other request is "method not found".

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

use crate::domain::loopback_message::RequestBody;
use crate::domain::mediums::{SERVER_NAME, tool_definitions};

use super::helper_way::{HelperWay, SERVER_GONE, ServerAnswer};

/// The protocol version answered when the agent names none.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Serves until the input ends, or until a tool call finds the server has ended.
pub fn serve_mcp(input: impl BufRead, mut output: impl Write, way: &HelperWay) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(Replied { reply, server }) = reply_to(&message, way) else {
            continue;
        };
        writeln!(output, "{reply}")?;
        output.flush()?;
        if server == Server::Gone {
            return Ok(());
        }
    }
    Ok(())
}

/// One reply, and whether the server was still there to give it.
struct Replied {
    reply: Value,
    server: Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Server {
    There,
    Gone,
}

/// The reply to one message; none for a notification.
fn reply_to(message: &Value, way: &HelperWay) -> Option<Replied> {
    let id = message.get("id")?.clone();
    let params = &message["params"];
    let (result, server) = match message["method"].as_str().unwrap_or_default() {
        "initialize" => (Ok(initialized(params)), Server::There),
        "tools/list" => (Ok(json!({ "tools": tool_list() })), Server::There),
        "tools/call" => {
            let (result, server) = called(params, way);
            (Ok(result), server)
        }
        "ping" => (Ok(json!({})), Server::There),
        other => (
            Err(json!({ "code": -32601, "message": format!("method not found: {other}") })),
            Server::There,
        ),
    };
    let reply = match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    };
    Some(Replied { reply, server })
}

fn initialized(params: &Value) -> Value {
    let version = params["protocolVersion"]
        .as_str()
        .unwrap_or(PROTOCOL_VERSION);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
    })
}

fn tool_list() -> Vec<Value> {
    tool_definitions()
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": (tool.schema)(),
            })
        })
        .collect()
}

/// One tool call, asked of the server; the answer is one text block.
fn called(params: &Value, way: &HelperWay) -> (Value, Server) {
    let answer = way.ask(RequestBody::Tool {
        name: params["name"].as_str().unwrap_or_default().to_string(),
        arguments: params.get("arguments").cloned().unwrap_or(json!({})),
    });
    let (text, is_error, server) = match answer {
        ServerAnswer::Reply(reply) => (reply.text, reply.is_error, Server::There),
        ServerAnswer::Gone => (SERVER_GONE.to_string(), true, Server::Gone),
    };
    let result = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    });
    (result, server)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn way() -> HelperWay {
        HelperWay {
            port: 1,
            token: "k".into(),
            task: "t".into(),
        }
    }

    #[test]
    fn initialize_and_the_tool_list_are_answered_without_the_server() {
        let input = [
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" }),
        ]
        .map(|m| m.to_string())
        .join("\n");
        let mut output = Vec::new();
        serve_mcp(input.as_bytes(), &mut output, &way()).unwrap();
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 7);
        assert_eq!(replies[2]["error"]["code"], -32601);
    }

    #[test]
    fn a_tool_call_to_a_server_that_has_ended_fails_and_the_server_exits() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let way = HelperWay {
            port: closed.local_addr().unwrap().port(),
            token: "k".into(),
            task: "t".into(),
        };
        drop(closed);
        let call = |id: u64| json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": "notify", "arguments": { "text": "hi" } } });
        let input = [call(1), call(2)].map(|m| m.to_string()).join("\n");
        let mut output = Vec::new();
        serve_mcp(input.as_bytes(), &mut output, &way).unwrap();
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 1, "the second call is never answered");
        assert_eq!(replies[0]["result"]["isError"], true);
        assert_eq!(replies[0]["result"]["content"][0]["text"], SERVER_GONE);
    }
}
