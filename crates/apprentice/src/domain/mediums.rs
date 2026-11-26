//! The agent's only way to the person: six tools, each a medium (a message once, a line in the
//! margin, grey text, a change in the buffer, a note beside the code, a question), plus the
//! permission tool `claude` asks before a command. Their names, descriptions and argument
//! shapes are what the MCP server lists; [`tool_call_of`] reads a call's arguments.

use serde_json::{Value, json};

/// How the tools are named to the agent: `mcp__apprentice__write_file`.
pub const TOOL_PREFIX: &str = "mcp__apprentice__";
/// The MCP server's name in the agent's config.
pub const SERVER_NAME: &str = "apprentice";
/// The permission tool's name without the prefix.
pub const PERMISSION_TOOL: &str = "permission";

/// One medium tool as the MCP server lists it.
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
}

/// The six mediums and the permission tool, in the order the server lists them.
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "notify",
            description: "Tell the person one short thing, once (a sentence). Use it for news that is about no line: you are done, a run passed.",
            schema: notify_schema,
        },
        ToolDefinition {
            name: "mark",
            description: "Put a line in the margin of a file, with hover text: for a sentence or a paragraph about that line (a finding, a test that fails, why you did not change something). Line counts from 1.",
            schema: mark_schema,
        },
        ToolDefinition {
            name: "ghost",
            description: "Show grey lines under a line of a file, not in the buffer: a line the person may want to type. They go at the person's next edit of that line.",
            schema: ghost_schema,
        },
        ToolDefinition {
            name: "write_file",
            description: "The only way to change a file: give its whole new text. It lands in the person's editor as one undoable change. When the file changed since you read it, the write is refused and you get its text now: rebase on it and write again.",
            schema: write_file_schema,
        },
        ToolDefinition {
            name: "note",
            description: "Write more than a paragraph (research findings, a plan, a long answer) as a markdown note opened beside the code. Cite code as path:line.",
            schema: note_schema,
        },
        ToolDefinition {
            name: "ask",
            description: "Ask the person to decide, with two to four options, and wait for the answer. how: actions (choices on a line, give file and line), picker (a list to pick from). After two minutes with no answer you are told to carry on with the default.",
            schema: ask_schema,
        },
        ToolDefinition {
            name: PERMISSION_TOOL,
            description: "Asks the person before a tool runs.",
            schema: permission_schema,
        },
    ]
}

fn notify_schema() -> Value {
    object(&[("text", text_field("the sentence"))], &["text"])
}

fn mark_schema() -> Value {
    object(
        &[
            ("file", text_field("the file, relative to the project root")),
            ("line", number_field("the line, from 1")),
            (
                "text",
                text_field("what to say; the first line shows in the margin, the rest on hover"),
            ),
            (
                "severity",
                json!({ "type": "string", "enum": ["information", "warning", "hint"] }),
            ),
        ],
        &["file", "line", "text"],
    )
}

fn ghost_schema() -> Value {
    object(
        &[
            ("file", text_field("the file, relative to the project root")),
            ("line", number_field("the line, from 1")),
            (
                "lines",
                json!({ "type": "array", "items": { "type": "string" } }),
            ),
        ],
        &["file", "line", "lines"],
    )
}

fn write_file_schema() -> Value {
    object(
        &[
            ("path", text_field("the file, relative to the project root")),
            ("text", text_field("the whole new text of the file")),
        ],
        &["path", "text"],
    )
}

fn note_schema() -> Value {
    object(
        &[
            ("title", text_field("a few words")),
            ("text", text_field("the note, in markdown")),
        ],
        &["title", "text"],
    )
}

fn ask_schema() -> Value {
    object(
        &[
            ("question", text_field("the question, one sentence")),
            (
                "options",
                json!({ "type": "array", "items": { "type": "string" }, "minItems": 2, "maxItems": 4 }),
            ),
            (
                "how",
                json!({ "type": "string", "enum": ["actions", "picker", "edit"] }),
            ),
            (
                "file",
                text_field("where to ask, relative to the root (for actions)"),
            ),
            ("line", number_field("the line, from 1")),
            (
                "default",
                text_field("the option you will take if no answer comes"),
            ),
        ],
        &["question", "options"],
    )
}

fn permission_schema() -> Value {
    object(
        &[
            ("tool_name", text_field("the tool")),
            ("input", json!({ "type": "object" })),
            ("tool_use_id", text_field("the call")),
        ],
        &["tool_name", "input"],
    )
}

fn object(properties: &[(&str, Value)], required: &[&str]) -> Value {
    let properties: serde_json::Map<String, Value> = properties
        .iter()
        .map(|(name, schema)| (name.to_string(), schema.clone()))
        .collect();
    json!({ "type": "object", "properties": properties, "required": required })
}

fn text_field(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn number_field(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

/// How a line in the margin looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkSeverity {
    Information,
    Warning,
    Hint,
}

/// How a question is put to the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskWay {
    /// One code action per option on the line.
    Actions,
    /// A list to pick from (`window/showMessageRequest`).
    Picker,
    /// A mark answered by editing the line; this prototype offers it as actions.
    Edit,
}

/// A question for the person.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub question: String,
    pub options: Vec<String>,
    pub how: AskWay,
    /// Where to ask: a file relative to the root and a line from 1.
    pub at: Option<(String, u32)>,
    pub default: Option<String>,
}

/// A medium tool call with its arguments read.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolCall {
    Notify {
        text: String,
    },
    Mark {
        file: String,
        line: u32,
        text: String,
        severity: MarkSeverity,
    },
    Ghost {
        file: String,
        line: u32,
        lines: Vec<String>,
    },
    WriteFile {
        path: String,
        text: String,
    },
    Note {
        title: String,
        text: String,
    },
    Ask(Ask),
}

/// The call of the tool named `name` (without the prefix), its arguments read; the sentence
/// to give back to the agent when a needed argument is missing or the tool is unknown.
///
/// Example: `tool_call_of("notify", {"text": "done"})` is `Notify { text: "done" }`;
/// `tool_call_of("notify", {})` is `Err("notify needs text")`.
pub fn tool_call_of(name: &str, arguments: &Value) -> Result<ToolCall, String> {
    let text = |key: &str| {
        arguments[key]
            .as_str()
            .map(String::from)
            .ok_or_else(|| format!("{name} needs {key}"))
    };
    let line = || {
        arguments["line"]
            .as_u64()
            .filter(|line| *line >= 1)
            .map(|line| line as u32)
            .ok_or_else(|| format!("{name} needs line, a number from 1"))
    };
    match name {
        "notify" => Ok(ToolCall::Notify {
            text: text("text")?,
        }),
        "mark" => Ok(ToolCall::Mark {
            file: text("file")?,
            line: line()?,
            text: text("text")?,
            severity: severity_of(&arguments["severity"]),
        }),
        "ghost" => Ok(ToolCall::Ghost {
            file: text("file")?,
            line: line()?,
            lines: strings_of(&arguments["lines"]),
        }),
        "write_file" => Ok(ToolCall::WriteFile {
            path: text("path")?,
            text: text("text")?,
        }),
        "note" => Ok(ToolCall::Note {
            title: text("title")?,
            text: text("text")?,
        }),
        "ask" => ask_of(arguments, text("question")?),
        other => Err(format!("there is no tool {other}")),
    }
}

fn ask_of(arguments: &Value, question: String) -> Result<ToolCall, String> {
    let options = strings_of(&arguments["options"]);
    if options.len() < 2 {
        return Err("ask needs two to four options".to_string());
    }
    let how = match arguments["how"].as_str() {
        Some("picker") => AskWay::Picker,
        Some("edit") => AskWay::Edit,
        _ => AskWay::Actions,
    };
    let at = arguments["file"].as_str().map(|file| {
        let line = arguments["line"].as_u64().unwrap_or(1).max(1) as u32;
        (file.to_string(), line)
    });
    Ok(ToolCall::Ask(Ask {
        question,
        options: options.into_iter().take(4).collect(),
        how,
        at,
        default: arguments["default"].as_str().map(String::from),
    }))
}

fn severity_of(value: &Value) -> MarkSeverity {
    match value.as_str() {
        Some("warning") => MarkSeverity::Warning,
        Some("hint") => MarkSeverity::Hint,
        _ => MarkSeverity::Information,
    }
}

fn strings_of(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

/// Whether a tool call (by its full name) speaks to the person through a medium.
pub fn is_a_medium(full_name: &str) -> bool {
    full_name
        .strip_prefix(TOOL_PREFIX)
        .is_some_and(|name| name != PERMISSION_TOOL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_schema_that_is_an_object() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 7);
        for tool in tools {
            assert_eq!((tool.schema)()["type"], "object", "{}", tool.name);
        }
    }

    #[test]
    fn a_call_reads_its_arguments_and_names_what_is_missing() {
        assert_eq!(
            tool_call_of("mark", &json!({ "file": "a.txt", "line": 3, "text": "x" })),
            Ok(ToolCall::Mark {
                file: "a.txt".into(),
                line: 3,
                text: "x".into(),
                severity: MarkSeverity::Information
            })
        );
        assert_eq!(
            tool_call_of("mark", &json!({ "file": "a.txt", "line": 0, "text": "x" })),
            Err("mark needs line, a number from 1".into())
        );
        assert_eq!(
            tool_call_of("write_file", &json!({ "path": "a" })),
            Err("write_file needs text".into())
        );
        assert_eq!(
            tool_call_of("shout", &json!({})),
            Err("there is no tool shout".into())
        );
    }

    #[test]
    fn an_ask_needs_two_options_and_takes_at_most_four() {
        assert!(tool_call_of("ask", &json!({ "question": "q", "options": ["a"] })).is_err());
        let Ok(ToolCall::Ask(ask)) = tool_call_of(
            "ask",
            &json!({ "question": "q", "options": ["a", "b", "c", "d", "e"], "how": "picker", "file": "x.txt", "line": 4 }),
        ) else {
            panic!();
        };
        assert_eq!(ask.options.len(), 4);
        assert_eq!(ask.how, AskWay::Picker);
        assert_eq!(ask.at, Some(("x.txt".into(), 4)));
    }

    #[test]
    fn the_permission_tool_is_no_medium() {
        assert!(is_a_medium("mcp__apprentice__notify"));
        assert!(!is_a_medium("mcp__apprentice__permission"));
        assert!(!is_a_medium("Read"));
    }
}
