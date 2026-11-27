//! The status line of a running task, derived from what the agent does, never written by it:
//! `apprentice: refuse late cancellations · reading booking.txt · $0.12`.

use std::path::Path;

use serde_json::Value;

use crate::domain::log_text::usd;
use crate::domain::mediums::TOOL_PREFIX;

/// What a tool call is doing, in a few words: `reading booking.txt`, `searching Slot`, `running
/// pytest`, `writing cancel_booking.txt`, `asking you`. A subagent's call is said as
/// `subagent: reading …`. Paths are named by their file name; `root` is cut from absolute ones.
///
/// Example: `Bash` with `{"command": "uv run pytest -q"}` is `running uv`.
pub fn activity_of(name: &str, input: &Value, parent: Option<&str>, root: &Path) -> String {
    let doing = doing_of(name, input, root);
    match parent {
        Some(_) => format!("subagent: {doing}"),
        None => doing,
    }
}

fn doing_of(name: &str, input: &Value, root: &Path) -> String {
    let field = |key: &str| input[key].as_str().unwrap_or_default().to_string();
    let file = |key: &str| file_name_of(&field(key), root);
    match name.strip_prefix(TOOL_PREFIX) {
        Some("write_file") => format!("writing {}", file("path")),
        Some("ask") | Some("permission") => "asking you".to_string(),
        Some("note") => "writing a note".to_string(),
        Some(_) => "telling you".to_string(),
        None => match name {
            "Read" => format!("reading {}", file("file_path")),
            "Grep" | "Glob" => format!("searching {}", field("pattern")),
            "Bash" => format!("running {}", first_word(&field("command"))),
            "Agent" | "Task" => "starting a subagent".to_string(),
            "WebSearch" => "searching the web".to_string(),
            "WebFetch" => "reading the web".to_string(),
            other => format!("using {other}"),
        },
    }
}

/// Where a command stands with the person: asked and not answered yet, allowed, or denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandVerdict {
    Waiting,
    Allowed,
    Denied,
}

/// What a command is doing once the person was asked about it, by its first word:
/// `waiting for you: ls`, `running ls`, `denied: ls`.
///
/// Example: `command_activity("ls -a && cat CLAUDE.md", CommandVerdict::Denied)` is `denied: ls`.
pub fn command_activity(command: &str, verdict: CommandVerdict) -> String {
    let program = first_word(command);
    match verdict {
        CommandVerdict::Waiting => format!("waiting for you: {program}"),
        CommandVerdict::Allowed => format!("running {program}"),
        CommandVerdict::Denied => format!("denied: {program}"),
    }
}

fn first_word(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .unwrap_or("a command")
        .to_string()
}

/// A path as the status line names it: its last part.
fn file_name_of(path: &str, root: &Path) -> String {
    let relative = Path::new(path)
        .strip_prefix(root)
        .unwrap_or(Path::new(path));
    relative
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// The status line: the task's title, what it does now, and what it has spent.
///
/// Example: `status_text("refuse late cancellations", "reading booking.txt", 0.12)` is
/// `refuse late cancellations · reading booking.txt · $0.12`.
pub fn status_text(title: &str, activity: &str, spent_usd: f64) -> String {
    format!("{title} · {activity} · {}", usd(spent_usd))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn each_tool_says_what_it_does() {
        let root = Path::new("/p");
        let cases = [
            (
                "Read",
                json!({ "file_path": "/p/src/booking.txt" }),
                "reading booking.txt",
            ),
            ("Grep", json!({ "pattern": "Slot" }), "searching Slot"),
            ("Bash", json!({ "command": "make test" }), "running make"),
            (
                "mcp__apprentice__write_file",
                json!({ "path": "src/a.txt" }),
                "writing a.txt",
            ),
            ("mcp__apprentice__ask", json!({}), "asking you"),
            ("Agent", json!({}), "starting a subagent"),
            ("TodoWrite", json!({}), "using TodoWrite"),
        ];
        for (name, input, said) in cases {
            assert_eq!(activity_of(name, &input, None, root), said, "{name}");
        }
        assert_eq!(
            activity_of(
                "Read",
                &json!({ "file_path": "docs/routes.md" }),
                Some("u1"),
                root
            ),
            "subagent: reading routes.md"
        );
    }

    #[test]
    fn a_command_asked_about_says_where_it_stands() {
        let command = "ls -a && cat CLAUDE.md";
        assert_eq!(
            command_activity(command, CommandVerdict::Waiting),
            "waiting for you: ls"
        );
        assert_eq!(
            command_activity(command, CommandVerdict::Allowed),
            "running ls"
        );
        assert_eq!(
            command_activity(command, CommandVerdict::Denied),
            "denied: ls"
        );
    }

    #[test]
    fn the_line_has_the_title_the_activity_and_the_cost() {
        assert_eq!(
            status_text("refuse late cancellations", "reading booking.txt", 0.12),
            "refuse late cancellations · reading booking.txt · $0.12"
        );
    }
}
