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
/// A command the stream shows has not been allowed yet: every command goes through the
/// permission tool first, which says `running …` once it is allowed.
///
/// Example: `Bash` with `{"command": "uv run pytest -q"}` is `waiting until allowed: uv`.
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
            "Bash" => format!("waiting until allowed: {}", programs_of(&field("command"))),
            "Agent" | "Task" => "starting a subagent".to_string(),
            "WebSearch" => "searching the web".to_string(),
            "WebFetch" => "reading the web".to_string(),
            // Claude Code loading its own tools' definitions: nothing the person asked about
            "ToolSearch" => "getting ready".to_string(),
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

/// What a command is doing once the person was asked about it, by its programs:
/// `waiting for you: ls`, `running ls`, `denied: ls`.
///
/// Example: `command_activity("ls -a && cat CLAUDE.md", CommandVerdict::Denied)` is `denied: ls,
/// cat`.
pub fn command_activity(command: &str, verdict: CommandVerdict) -> String {
    let programs = programs_of(command);
    match verdict {
        CommandVerdict::Waiting => format!("waiting for you: {programs}"),
        CommandVerdict::Allowed => format!("running {programs}"),
        CommandVerdict::Denied => format!("denied: {programs}"),
    }
}

/// at most this many programs of a chain are named; the rest is `…`
const PROGRAMS_NAMED: usize = 3;

/// The programs a command runs, in order and each once: the first word of each part of a chain
/// (split at `;`, `&&`, `||`, `|`, `&` and new lines), past any `NAME=value`, without brackets.
/// At most 3 are named.
///
/// Example: `ls; cat pyproject.toml && FOO=1 uv run pytest | tail` is `ls, cat, uv, …`, and
/// `ls -a` is `ls`.
fn programs_of(command: &str) -> String {
    let mut programs: Vec<&str> = Vec::new();
    for part in command.split([';', '&', '|', '\n']) {
        let program = part
            .split_whitespace()
            .map(|word| word.trim_matches(['(', '{', ')', '}']))
            .find(|word| !word.is_empty() && !is_assignment(word));
        if let Some(program) = program
            && !programs.contains(&program)
        {
            programs.push(program);
        }
    }
    match programs.len() {
        0 => "a command".to_string(),
        n if n <= PROGRAMS_NAMED => programs.join(", "),
        _ => format!("{}, \u{2026}", programs[..PROGRAMS_NAMED].join(", ")),
    }
}

/// `NAME=value` before a program: an environment variable set for it, not the program.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
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
            (
                "Bash",
                json!({ "command": "make test" }),
                "waiting until allowed: make",
            ),
            (
                "mcp__apprentice__write_file",
                json!({ "path": "src/a.txt" }),
                "writing a.txt",
            ),
            ("mcp__apprentice__ask", json!({}), "asking you"),
            ("Agent", json!({}), "starting a subagent"),
            ("TodoWrite", json!({}), "using TodoWrite"),
            (
                "ToolSearch",
                json!({ "query": "select:x" }),
                "getting ready",
            ),
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
            "waiting for you: ls, cat"
        );
        assert_eq!(
            command_activity(command, CommandVerdict::Allowed),
            "running ls, cat"
        );
        assert_eq!(
            command_activity(command, CommandVerdict::Denied),
            "denied: ls, cat"
        );
    }

    #[test]
    fn a_chain_is_said_as_its_programs_without_its_separators() {
        let said = |command| command_activity(command, CommandVerdict::Denied);
        assert_eq!(said("ls; cat pyproject.toml"), "denied: ls, cat");
        assert_eq!(said("ls -a"), "denied: ls");
        assert_eq!(said("cat a | grep b | grep c"), "denied: cat, grep");
        assert_eq!(
            said("FOO=1 make test || (cd src && ls)"),
            "denied: make, cd, ls"
        );
        assert_eq!(said("a; b; c; d"), "denied: a, b, c, \u{2026}");
        assert_eq!(said(" ; "), "denied: a command");
    }

    #[test]
    fn the_line_has_the_title_the_activity_and_the_cost() {
        assert_eq!(
            status_text("refuse late cancellations", "reading booking.txt", 0.12),
            "refuse late cancellations · reading booking.txt · $0.12"
        );
    }
}
