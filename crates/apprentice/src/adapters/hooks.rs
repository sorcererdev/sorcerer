//! `apprentice hook post-tool-use|stop`: the hooks the agent runs. Each asks the server for the
//! person's edits since the task last looked and prints what the agent should be told; a hook
//! never fails the agent (a server that cannot be reached prints nothing).

use serde_json::json;

use crate::domain::loopback_message::RequestBody;

use super::helper_way::HelperWay;

/// The hook's output for an event (`post-tool-use`, `stop`): after a tool call, the edits as
/// `additionalContext`; nothing when there are none, or at the end of the turn.
///
/// Example: after a `Read`, with `a.txt` edited by the person, the output is
/// `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"The person changed…"}}`.
pub fn hook_output(event: &str, way: &HelperWay) -> String {
    let reply = way.ask(RequestBody::Edits {
        event: event.to_string(),
    });
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
    fn a_server_that_cannot_be_reached_prints_nothing() {
        let way = HelperWay {
            port: 9,
            token: "k".into(),
            task: "t".into(),
        };
        assert_eq!(hook_output("post-tool-use", &way), "");
    }
}
