//! The loop researcher's three tools, in Rust, under fences: `read` (with
//! a line range), `grep` and `glob`, over a [`CodebaseView`]. A fenced path
//! is refused with one line and never read; fenced hits are dropped and
//! counted. Output is capped so one call cannot flood the context.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::ports::view::CodebaseView;

use super::Tool;
use crate::provider::{ToolCall, ToolSpec};

pub const READ_LINES: usize = 400;
pub const GREP_HITS: usize = 100;
pub const GREP_WIDTH: usize = 200;
pub const GLOB_PATHS: usize = 200;

/// The specs of TOOLS, as the model is offered them.
pub fn specs(tools: &[Tool]) -> Vec<ToolSpec> {
    tools
        .iter()
        .map(|t| match t {
            Tool::Read => ToolSpec {
                name: "read".into(),
                description: format!("Read a file of the project, with line numbers (at most {READ_LINES} lines; ask for a range for more)."),
                schema: json!({"type": "object", "properties": {
                    "path": {"type": "string", "description": "relative to the project root"},
                    "start_line": {"type": "integer"}, "end_line": {"type": "integer"}
                }, "required": ["path"]}),
            },
            Tool::Grep => ToolSpec {
                name: "grep".into(),
                description: format!("Search the project's files for a regular expression; at most {GREP_HITS} hits as path:line: text."),
                schema: json!({"type": "object", "properties": {
                    "pattern": {"type": "string"}, "glob": {"type": "string", "description": "only files matching this glob"}
                }, "required": ["pattern"]}),
            },
            Tool::Glob => ToolSpec {
                name: "glob".into(),
                description: format!("List the project's files matching a glob such as src/**/*.py; at most {GLOB_PATHS}."),
                schema: json!({"type": "object", "properties": {"pattern": {"type": "string"}}, "required": ["pattern"]}),
            },
        })
        .collect()
}

/// Runs CALL; returns what the model sees and the path read, if any.
pub fn run(call: &ToolCall, view: &dyn CodebaseView, fences: &Fences) -> (String, Option<PathBuf>) {
    let s = |k: &str| call.input.get(k).and_then(Value::as_str);
    match call.name.as_str() {
        "read" => match s("path") {
            Some(p) => read(
                Path::new(p),
                call.input.get("start_line").and_then(Value::as_u64),
                call.input.get("end_line").and_then(Value::as_u64),
                view,
                fences,
            ),
            None => ("error: read needs a path".into(), None),
        },
        "grep" => match s("pattern") {
            Some(p) => (grep(p, s("glob"), view, fences), None),
            None => ("error: grep needs a pattern".into(), None),
        },
        "glob" => match s("pattern") {
            Some(p) => (glob(p, view, fences), None),
            None => ("error: glob needs a pattern".into(), None),
        },
        other => (format!("error: no tool named {other}"), None),
    }
}

fn read(
    path: &Path,
    start: Option<u64>,
    end: Option<u64>,
    view: &dyn CodebaseView,
    fences: &Fences,
) -> (String, Option<PathBuf>) {
    let refused = (
        format!("refused: {} is fenced (not evidence)", path.display()),
        None,
    );
    if !fences.allows(path) {
        return refused;
    }
    let Some(rel) = fences.relative(path) else {
        return refused;
    };
    let text = match view.text(&rel) {
        Ok(t) => t,
        Err(e) => return (format!("error: {e}"), None),
    };
    let first = start.unwrap_or(1).max(1) as usize;
    let last = end.map_or(usize::MAX, |e| e as usize);
    let mut lines: Vec<String> = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(n, _)| *n >= first && *n <= last)
        .map(|(n, l)| format!("{n}\t{l}"))
        .collect();
    if lines.len() > READ_LINES {
        lines.truncate(READ_LINES);
        lines.push(format!(
            "… (truncated at {READ_LINES} lines; ask for a range)"
        ));
    }
    (lines.join("\n"), Some(rel))
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

fn grep(pattern: &str, glob: Option<&str>, view: &dyn CodebaseView, fences: &Fences) -> String {
    let hits = match view.grep(pattern, glob) {
        Ok(h) => h,
        Err(e) => return format!("error: {e}"),
    };
    let (allowed, fenced): (Vec<_>, Vec<_>) =
        hits.into_iter().partition(|h| fences.allows(&h.path));
    let mut out: Vec<String> = allowed
        .iter()
        .take(GREP_HITS)
        .map(|h| {
            format!(
                "{}:{}: {}",
                h.path.display(),
                h.line,
                h.text.chars().take(GREP_WIDTH).collect::<String>()
            )
        })
        .collect();
    if allowed.len() > GREP_HITS {
        out.push(format!(
            "… ({} more hits not shown; narrow the pattern or the glob)",
            allowed.len() - GREP_HITS
        ));
    }
    if out.is_empty() {
        out.push("no hits".into());
    }
    if !fenced.is_empty() {
        out.push(format!(
            "({} in fenced paths not shown)",
            plural(fenced.len(), "hit")
        ));
    }
    out.join("\n")
}

fn glob(pattern: &str, view: &dyn CodebaseView, fences: &Fences) -> String {
    let paths = match view.glob(pattern) {
        Ok(p) => p,
        Err(e) => return format!("error: {e}"),
    };
    let allowed: Vec<_> = paths.into_iter().filter(|p| fences.allows(p)).collect();
    let mut out: Vec<String> = allowed
        .iter()
        .take(GLOB_PATHS)
        .map(|p| p.display().to_string())
        .collect();
    if allowed.len() > GLOB_PATHS {
        out.push(format!("… ({} more not shown)", allowed.len() - GLOB_PATHS));
    }
    if out.is_empty() {
        out.push("no files".into());
    }
    out.join("\n")
}
