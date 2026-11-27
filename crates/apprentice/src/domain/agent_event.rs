//! What the agent tells us while it works, read from its stream of JSON lines: the tools it
//! calls (its own and its subagents'), the text it writes, and the end of its turn with what it
//! cost. Only these are read; every other line (the system's, the tools' results) is nothing.

use serde_json::Value;

/// One thing the agent did.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// The session started; its id lets a later run resume it.
    Started { session_id: String },
    /// The agent (or a subagent, when `parent` names the call that started it) called a tool.
    ToolUse {
        name: String,
        input: Value,
        parent: Option<String>,
    },
    /// The agent wrote text (outside any tool).
    Text {
        text: String,
        parent: Option<String>,
    },
    /// The turn ended.
    Finished(Finished),
}

/// The end of a turn, as the agent reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    /// The turn's total cost in dollars, as the agent estimates it.
    pub cost_usd: Option<f64>,
    pub session_id: Option<String>,
    /// Whether the agent says the turn failed (a cap reached, an error).
    pub is_error: bool,
    /// The turn's final text.
    pub text: String,
    pub turns: u32,
}

/// The events one line of the stream holds: a message of the assistant may hold several tool
/// calls and texts; the result holds the end; anything else, or a line that is not JSON, holds
/// none.
///
/// Example: `{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read",
/// "input":{"file_path":"a.txt"}}]},"parent_tool_use_id":null}` is one `ToolUse` of `Read`.
pub fn events_of_line(line: &str) -> Vec<AgentEvent> {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match message["type"].as_str() {
        Some("system") if message["subtype"] == "init" => started(&message).into_iter().collect(),
        Some("assistant") => assistant_events(&message),
        Some("result") => vec![AgentEvent::Finished(finished(&message))],
        _ => Vec::new(),
    }
}

fn started(message: &Value) -> Option<AgentEvent> {
    let session_id = message["session_id"].as_str()?.to_string();
    Some(AgentEvent::Started { session_id })
}

fn assistant_events(message: &Value) -> Vec<AgentEvent> {
    let parent = message["parent_tool_use_id"].as_str().map(String::from);
    let blocks = message["message"]["content"].as_array();
    blocks
        .into_iter()
        .flatten()
        .filter_map(|block| match block["type"].as_str() {
            Some("tool_use") => Some(AgentEvent::ToolUse {
                name: block["name"].as_str().unwrap_or_default().to_string(),
                input: block["input"].clone(),
                parent: parent.clone(),
            }),
            Some("text") => {
                let text = block["text"].as_str().unwrap_or_default().trim();
                (!text.is_empty()).then(|| AgentEvent::Text {
                    text: text.to_string(),
                    parent: parent.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

fn finished(message: &Value) -> Finished {
    Finished {
        cost_usd: message["total_cost_usd"].as_f64(),
        session_id: message["session_id"].as_str().map(String::from),
        is_error: message["is_error"].as_bool().unwrap_or(false),
        text: message["result"].as_str().unwrap_or_default().to_string(),
        turns: message["num_turns"].as_u64().unwrap_or(0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_message_holds_its_tool_calls_and_its_text() {
        let line = json!({
            "type": "assistant",
            "parent_tool_use_id": null,
            "message": { "content": [
                { "type": "text", "text": "Looking at the booking.  " },
                { "type": "tool_use", "id": "u1", "name": "Read", "input": { "file_path": "a.txt" } },
            ]},
        })
        .to_string();
        assert_eq!(
            events_of_line(&line),
            [
                AgentEvent::Text {
                    text: "Looking at the booking.".into(),
                    parent: None
                },
                AgentEvent::ToolUse {
                    name: "Read".into(),
                    input: json!({ "file_path": "a.txt" }),
                    parent: None
                },
            ]
        );
    }

    #[test]
    fn a_subagent_call_names_its_parent() {
        let line = json!({
            "type": "assistant",
            "parent_tool_use_id": "u7",
            "message": { "content": [{ "type": "tool_use", "name": "Grep", "input": {} }] },
        })
        .to_string();
        let [AgentEvent::ToolUse { parent, .. }] = &events_of_line(&line)[..] else {
            panic!();
        };
        assert_eq!(parent.as_deref(), Some("u7"));
    }

    #[test]
    fn the_result_ends_the_turn_with_its_cost() {
        let line = json!({
            "type": "result", "subtype": "success", "is_error": false, "result": "done",
            "total_cost_usd": 0.71, "num_turns": 9, "session_id": "s1",
        })
        .to_string();
        assert_eq!(
            events_of_line(&line),
            [AgentEvent::Finished(Finished {
                cost_usd: Some(0.71),
                session_id: Some("s1".into()),
                is_error: false,
                text: "done".into(),
                turns: 9,
            })]
        );
    }

    #[test]
    fn the_start_names_the_session_and_other_lines_are_nothing() {
        let init = json!({ "type": "system", "subtype": "init", "session_id": "s1" }).to_string();
        assert_eq!(
            events_of_line(&init),
            [AgentEvent::Started {
                session_id: "s1".into()
            }]
        );
        assert!(events_of_line(r#"{"type":"user","message":{}}"#).is_empty());
        assert!(events_of_line("not json").is_empty());
    }
}
