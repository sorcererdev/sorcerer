//! The loop researcher's three tools, in Rust, under fences: `read` (with
//! a line range), `grep` and `glob`, over a [`CodebaseView`]. A fenced path
//! is refused with one line and never read; fenced hits are dropped and
//! counted. Output is capped so one call cannot flood the context.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::model::{ToolCall, ToolSpec};
use crate::domain::research::Tool;

pub const READ_LINES: usize = 400;
pub const GREP_HITS: usize = 100;
pub const GREP_WIDTH: usize = 200;
pub const GLOB_PATHS: usize = 200;

/// The tools as the model is offered them: a name, what it does, and its input.
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

/// What a tool gave back: the text the model sees, and the file it read, if any (a
/// research lists the files it read).
pub struct ToolOutput {
    pub text: String,
    pub read: Option<PathBuf>,
}

impl ToolOutput {
    /// Output that read no file: an error, a refusal, or a grep or glob listing.
    fn text(text: impl Into<String>) -> ToolOutput {
        ToolOutput {
            text: text.into(),
            read: None,
        }
    }
}

/// Runs the tool the model asked for. A wrong call (no such tool, or a required input
/// missing) gets one line saying so, for the model to correct on its next turn.
pub fn run(call: &ToolCall, view: &dyn CodebaseView, fences: &Fences) -> ToolOutput {
    let text_input = |name: &str| call.input.get(name).and_then(Value::as_str);
    let line_input = |name: &str| call.input.get(name).and_then(Value::as_u64);
    match call.name.as_str() {
        "read" => match text_input("path") {
            Some(path) => {
                let lines = Lines {
                    first: line_input("start_line"),
                    last: line_input("end_line"),
                };
                read(Path::new(path), lines, view, fences)
            }
            None => ToolOutput::text("error: read needs a path"),
        },
        "grep" => match text_input("pattern") {
            Some(pattern) => ToolOutput::text(grep(pattern, text_input("glob"), view, fences)),
            None => ToolOutput::text("error: grep needs a pattern"),
        },
        "glob" => match text_input("pattern") {
            Some(pattern) => ToolOutput::text(glob(pattern, view, fences)),
            None => ToolOutput::text("error: glob needs a pattern"),
        },
        other => ToolOutput::text(format!("error: no tool named {other}")),
    }
}

/// The lines of a file the model asked for, counted from 1; the whole file when it gave
/// neither end.
struct Lines {
    first: Option<u64>,
    last: Option<u64>,
}

impl Lines {
    fn includes(&self, number: usize) -> bool {
        let first = self.first.unwrap_or(1).max(1) as usize;
        let last = self.last.map_or(usize::MAX, |last| last as usize);
        (first..=last).contains(&number)
    }
}

/// The asked-for lines of a file, each with its number, at most 400 of them. A fenced
/// file, or one outside the project, is refused without being read.
///
/// Example: lines 10 to 12 of `app.py` come back as `10\t...`, `11\t...`, `12\t...`.
fn read(path: &Path, lines: Lines, view: &dyn CodebaseView, fences: &Fences) -> ToolOutput {
    let Some(relative) = fences.relative(path).filter(|_| fences.allows(path)) else {
        return ToolOutput::text(format!(
            "refused: {} is fenced (not evidence)",
            path.display()
        ));
    };
    let text = match view.text(&relative) {
        Ok(text) => text,
        Err(e) => return ToolOutput::text(format!("error: {e}")),
    };
    let numbered: Vec<String> = text
        .lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line))
        .filter(|(number, _)| lines.includes(*number))
        .map(|(number, line)| format!("{number}\t{line}"))
        .collect();
    let shown = first_of(numbered, READ_LINES, |_| {
        format!("… (truncated at {READ_LINES} lines; ask for a range)")
    });
    ToolOutput {
        text: shown.join("\n"),
        read: Some(relative),
    }
}

/// The hits as `path:line: text`, at most 100, each text cut at 200 characters. Hits in
/// fenced files are not shown, only counted.
///
/// Example: 3 hits in `app.py` and 1 in `.env` show the three and end with
/// "(1 hit in fenced paths not shown)".
fn grep(pattern: &str, glob: Option<&str>, view: &dyn CodebaseView, fences: &Fences) -> String {
    let hits = match view.grep(pattern, glob) {
        Ok(hits) => hits,
        Err(e) => return format!("error: {e}"),
    };
    let (allowed, fenced): (Vec<_>, Vec<_>) =
        hits.into_iter().partition(|hit| fences.allows(&hit.path));
    let lines: Vec<String> = allowed
        .iter()
        .map(|hit| {
            format!(
                "{}:{}: {}",
                hit.path.display(),
                hit.line,
                hit.text.chars().take(GREP_WIDTH).collect::<String>()
            )
        })
        .collect();
    let shown = first_of(lines, GREP_HITS, |more| {
        format!("… ({more} more hits not shown; narrow the pattern or the glob)")
    });
    let shown = if shown.is_empty() {
        vec!["no hits".to_string()]
    } else {
        shown
    };
    let fenced_note = (!fenced.is_empty()).then(|| {
        format!(
            "({} in fenced paths not shown)",
            plural(fenced.len(), "hit")
        )
    });
    shown
        .into_iter()
        .chain(fenced_note)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The project's files matching the glob, at most 200; fenced files are left out.
fn glob(pattern: &str, view: &dyn CodebaseView, fences: &Fences) -> String {
    let paths = match view.glob(pattern) {
        Ok(paths) => paths,
        Err(e) => return format!("error: {e}"),
    };
    let allowed: Vec<String> = paths
        .into_iter()
        .filter(|path| fences.allows(path))
        .map(|path| path.display().to_string())
        .collect();
    let shown = first_of(allowed, GLOB_PATHS, |more| {
        format!("… ({more} more not shown)")
    });
    if shown.is_empty() {
        "no files".into()
    } else {
        shown.join("\n")
    }
}

/// The first lines up to the cap; past it, one more line saying how many were left out.
fn first_of(lines: Vec<String>, cap: usize, left_out: impl Fn(usize) -> String) -> Vec<String> {
    if lines.len() <= cap {
        return lines;
    }
    let note = left_out(lines.len() - cap);
    lines.into_iter().take(cap).chain([note]).collect()
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<String> {
        (1..=n).map(|i| i.to_string()).collect()
    }

    #[test]
    fn first_of_keeps_lines_up_to_the_cap() {
        assert_eq!(first_of(lines(2), 2, |more| format!("{more} more")), lines(2));
    }

    #[test]
    fn first_of_says_how_many_were_left_out() {
        assert_eq!(
            first_of(lines(5), 2, |more| format!("{more} more")),
            vec!["1", "2", "3 more"]
        );
    }
}
