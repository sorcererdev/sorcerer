//! `apprentice hook post-tool-use|stop`: the hooks the agent runs. Each asks the server for the
//! person's edits since the task last looked and prints what the agent should be told; a hook
//! never fails the agent (a server that cannot be reached prints nothing), except when the
//! server has ended: then it stops the agent, so a task does not outlive its editor.

use serde_json::json;

use crate::domain::loopback_message::RequestBody;

use super::helper_way::{HelperWay, SERVER_GONE, ServerAnswer};

/// The hook's output for an event (`post-tool-use`, `stop`): after a tool call, the edits as
/// `additionalContext`; nothing when there are none, or at the end of the turn. When the
/// server has ended, `{"continue":false,…}`, which makes the agent stop after this tool call.
///
/// Example: after a `Read`, with `a.txt` edited by the person, the output is
/// `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"The person changed…"}}`.
pub fn hook_output(event: &str, way: &HelperWay) -> String {
    let answer = way.ask(RequestBody::Edits {
        event: event.to_string(),
    });
    let reply = match answer {
        ServerAnswer::Reply(reply) => reply,
        ServerAnswer::Gone => return stop_output(),
    };
    if reply.is_error || reply.text.is_empty() {
        return String::new();
    }
    json!({
        "hookSpecificOutput": {
            "hookEventName": hook_event_name(event),
            "additionalContext": reply.text,
        }
    })
    .to_string()
}

/// What stops the agent: a hook that answers `continue: false` ends its work after the call.
fn stop_output() -> String {
    json!({ "continue": false, "stopReason": SERVER_GONE }).to_string()
}

/// The event's name in the hook's output: `post-tool-use` is `PostToolUse`.
fn hook_event_name(event: &str) -> String {
    event
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_are_the_agents() {
        assert_eq!(hook_event_name("post-tool-use"), "PostToolUse");
        assert_eq!(hook_event_name("stop"), "Stop");
    }

    #[test]
    fn a_server_that_has_ended_stops_the_agent() {
        let way = HelperWay {
            port: closed_port(),
            token: "k".into(),
            task: "t".into(),
        };
        let output: serde_json::Value =
            serde_json::from_str(&hook_output("post-tool-use", &way)).unwrap();
        assert_eq!(output["continue"], false);
        assert_eq!(output["stopReason"], SERVER_GONE);
    }

    /// A port nothing listens on: one the system just gave out, closed again.
    fn closed_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }
}
