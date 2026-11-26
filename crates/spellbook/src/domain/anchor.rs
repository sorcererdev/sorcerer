//! Where an `insert` edit's text goes: a short phrase, `<position> <target>`, olifant's words.
//! `end of parameters in CloseTable.__init__` is after the last parameter of that method;
//! `before marker # routes` is above the line that reads `# routes`.

use regex::Regex;

/// A phrase read: where, relative to what, and the phrase as written (for the sentences).
///
/// Example: `end of call CloseTable in command` is `{ position: End, target: Call { name:
/// "CloseTable", within: Some("command") } }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub phrase: String,
    pub position: Position,
    pub target: Target,
}

/// Where the text goes relative to the target: inside it before its first item (`start of`) or
/// after its last (`end of`), or outside it, above (`before`) or below (`after`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Start,
    End,
    Before,
    After,
}

/// What a phrase points at. The first four need the file's grammar; the rest read the text alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The file's top-level imports.
    Imports,
    /// A function or method by name; a method is `Class.method`.
    Function(String),
    Class(String),
    /// The parameter list of a function or method.
    ParametersIn(String),
    /// The argument list of a call by the callee's name, inside one function when `within` names it.
    Call {
        name: String,
        within: Option<String>,
    },
    /// The line whose trimmed text is exactly this.
    Marker(String),
    FirstLineMatching(String),
    LastLineMatching(String),
    File,
}

/// The targets a phrase may name, for the sentence that refuses an unknown one.
const TARGETS: &str = "imports, function NAME, class NAME, parameters in FUNCTION, call NAME, call NAME in FUNCTION, marker TEXT, first line matching REGEX, last line matching REGEX, file";

impl Anchor {
    /// Reads a phrase. A target the grammar has no words for, a position the target does not
    /// take (`end of marker …`, `before file`) or a regex that does not compile is refused with a
    /// sentence naming the phrase.
    ///
    /// Example: `Anchor::parse("end of cases in update")` is refused: "`end of cases in
    /// update`: unknown target `cases in update` (targets: imports, function NAME, …)".
    pub fn parse(phrase: &str) -> Result<Anchor, String> {
        let trimmed = phrase.trim();
        let (position, rest) = position_of(trimmed).ok_or_else(|| {
            anchor_text(
                trimmed,
                "a phrase starts with `start of`, `end of`, `before` or `after`",
            )
        })?;
        let target = target_of(rest.trim()).map_err(|why| anchor_text(trimmed, &why))?;
        refuse_a_position_the_target_lacks(position, &target)
            .map_err(|why| anchor_text(trimmed, &why))?;
        Ok(Anchor {
            phrase: trimmed.to_string(),
            position,
            target,
        })
    }
}

/// Where a phrase put text, or why it put it nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    At(Place),
    /// Nothing in the file is the target: `what` names it ("function `CloseTable.__init__`").
    Missing {
        what: String,
    },
    /// The target is more than one place: `what` names it, `count` says how many.
    Several {
        what: String,
        count: usize,
    },
    /// The target is there but the position cannot be had: why.
    Refused {
        why: String,
    },
}

/// Where the text goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// New lines put before `row` (from 0; the row count is the end of the file), each indented
    /// by `indent` and the last ending with `suffix` (a list's separator, or nothing).
    Lines {
        row: usize,
        indent: String,
        suffix: String,
    },
    /// The text joins line `row` at byte `column`, written as `prefix`, the text, `suffix`: an
    /// item added to a list written on one line (`prefix` `, `).
    InLine {
        row: usize,
        column: usize,
        prefix: String,
        suffix: String,
    },
}

/// The sentence for a phrase that put text nowhere, in the file named `file_name`.
///
/// Example: a missing `function CloseTable.__init__` in `close_table.txt` gives "`end of
/// parameters in CloseTable.__init__`: no function `CloseTable.__init__` in close_table.txt".
pub fn resolution_text(anchor: &Anchor, file_name: &str, resolution: &Resolution) -> String {
    let why = match resolution {
        Resolution::At(_) => "resolves".to_string(),
        Resolution::Missing { what } => format!("no {what} in {file_name}"),
        Resolution::Several { what, count } => {
            format!("{count} places are {what} in {file_name}; narrow it with `in FUNCTION`")
        }
        Resolution::Refused { why } => why.clone(),
    };
    anchor_text(&anchor.phrase, &why)
}

/// Where a phrase whose target needs no grammar puts text; `None` for a target that needs the
/// file's grammar.
///
/// Example: `before marker # routes` in `a\n# routes\n` is new lines at row 1.
pub fn resolve_in_text(anchor: &Anchor, text: &str) -> Option<Resolution> {
    let lines: Vec<&str> = text.lines().collect();
    let line_at = |row: usize| {
        Resolution::At(Place::Lines {
            row: if anchor.position == Position::Before {
                row
            } else {
                row + 1
            },
            indent: indent_of(lines[row]),
            suffix: String::new(),
        })
    };
    let rows_where = |test: &dyn Fn(&str) -> bool| -> Vec<usize> {
        (0..lines.len()).filter(|row| test(lines[*row])).collect()
    };
    match &anchor.target {
        Target::File => Some(Resolution::At(Place::Lines {
            row: if anchor.position == Position::Start {
                0
            } else {
                lines.len()
            },
            indent: String::new(),
            suffix: String::new(),
        })),
        Target::Marker(marker) => {
            let rows = rows_where(&|line| line.trim() == marker);
            Some(match rows.as_slice() {
                [] => Resolution::Missing {
                    what: format!("line `{marker}`"),
                },
                [row] => line_at(*row),
                _ => Resolution::Several {
                    what: format!("the line `{marker}`"),
                    count: rows.len(),
                },
            })
        }
        Target::FirstLineMatching(pattern) | Target::LastLineMatching(pattern) => {
            let regex = regex::Regex::new(pattern).ok()?;
            let rows = rows_where(&|line| regex.is_match(line));
            let first = matches!(anchor.target, Target::FirstLineMatching(_));
            let row = if first { rows.first() } else { rows.last() };
            Some(match row {
                Some(row) => line_at(*row),
                None => Resolution::Missing {
                    what: format!("line matching `{pattern}`"),
                },
            })
        }
        _ => None,
    }
}

/// The spaces and tabs a line starts with.
pub fn indent_of(line: &str) -> String {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// A sentence about a phrase: the phrase quoted, then why.
///
/// Example: `anchor_text("end of function main", "no function `main` in app.txt")` is
/// "`end of function main`: no function `main` in app.txt".
pub fn anchor_text(phrase: &str, why: &str) -> String {
    format!("`{phrase}`: {why}")
}

fn position_of(phrase: &str) -> Option<(Position, &str)> {
    [
        ("start of ", Position::Start),
        ("end of ", Position::End),
        ("before ", Position::Before),
        ("after ", Position::After),
    ]
    .into_iter()
    .find_map(|(words, position)| phrase.strip_prefix(words).map(|rest| (position, rest)))
}

fn target_of(text: &str) -> Result<Target, String> {
    let named = |rest: &str| -> Result<String, String> {
        let name = rest.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(format!("`{text}` names one thing, without spaces"));
        }
        Ok(name.to_string())
    };
    let pattern = |rest: &str| -> Result<String, String> {
        Regex::new(rest).map_err(|e| format!("the regex `{rest}` does not compile ({e})"))?;
        Ok(rest.to_string())
    };
    if text == "imports" {
        return Ok(Target::Imports);
    }
    if text == "file" {
        return Ok(Target::File);
    }
    if let Some(rest) = text.strip_prefix("marker ") {
        return Ok(Target::Marker(rest.trim().to_string()));
    }
    if let Some(rest) = text.strip_prefix("first line matching ") {
        return pattern(rest).map(Target::FirstLineMatching);
    }
    if let Some(rest) = text.strip_prefix("last line matching ") {
        return pattern(rest).map(Target::LastLineMatching);
    }
    if let Some(rest) = text.strip_prefix("function ") {
        return named(rest).map(Target::Function);
    }
    if let Some(rest) = text.strip_prefix("class ") {
        return named(rest).map(Target::Class);
    }
    if let Some(rest) = text.strip_prefix("parameters in ") {
        return named(rest).map(Target::ParametersIn);
    }
    if let Some(rest) = text.strip_prefix("call ") {
        return match rest.split_once(" in ") {
            Some((name, within)) => Ok(Target::Call {
                name: named(name)?,
                within: Some(named(within)?),
            }),
            None => Ok(Target::Call {
                name: named(rest)?,
                within: None,
            }),
        };
    }
    Err(format!("unknown target `{text}` (targets: {TARGETS})"))
}

/// `marker` and `line matching` are lines, so the text goes `before` or `after` them; `file` has
/// no outside, so the text goes at its `start of` or `end of`.
fn refuse_a_position_the_target_lacks(position: Position, target: &Target) -> Result<(), String> {
    let inside = matches!(position, Position::Start | Position::End);
    match target {
        Target::Marker(_) | Target::FirstLineMatching(_) | Target::LastLineMatching(_)
            if inside =>
        {
            Err("a line has no inside: write `before` or `after`".into())
        }
        Target::File if !inside => {
            Err("a file has no outside: write `start of file` or `end of file`".into())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(phrase: &str) -> (Position, Target) {
        let anchor = Anchor::parse(phrase).unwrap();
        (anchor.position, anchor.target)
    }

    #[test]
    fn every_phrase_of_the_design_reads() {
        assert_eq!(target("end of imports"), (Position::End, Target::Imports));
        assert_eq!(
            target("end of parameters in CloseTable.__init__"),
            (
                Position::End,
                Target::ParametersIn("CloseTable.__init__".into())
            )
        );
        assert_eq!(
            target("end of function CloseTable.__init__"),
            (
                Position::End,
                Target::Function("CloseTable.__init__".into())
            )
        );
        assert_eq!(
            target("start of class CloseTable"),
            (Position::Start, Target::Class("CloseTable".into()))
        );
        assert_eq!(
            target("end of call CloseTable in command"),
            (
                Position::End,
                Target::Call {
                    name: "CloseTable".into(),
                    within: Some("command".into())
                }
            )
        );
        assert_eq!(
            target("end of call CloseTable"),
            (
                Position::End,
                Target::Call {
                    name: "CloseTable".into(),
                    within: None
                }
            )
        );
        assert_eq!(
            target("before marker # routes"),
            (Position::Before, Target::Marker("# routes".into()))
        );
        assert_eq!(
            target("after first line matching ^import"),
            (Position::After, Target::FirstLineMatching("^import".into()))
        );
        assert_eq!(
            target("after last line matching ^from \\."),
            (
                Position::After,
                Target::LastLineMatching("^from \\.".into())
            )
        );
        assert_eq!(target("start of file"), (Position::Start, Target::File));
        assert_eq!(target("end of file"), (Position::End, Target::File));
    }

    fn place(phrase: &str, text: &str) -> Resolution {
        resolve_in_text(&Anchor::parse(phrase).unwrap(), text).unwrap()
    }

    fn lines_at(row: usize, indent: &str) -> Resolution {
        Resolution::At(Place::Lines {
            row,
            indent: indent.into(),
            suffix: String::new(),
        })
    }

    #[test]
    fn targets_without_a_grammar_resolve_over_the_text() {
        let text = "a\n  # routes\nb\nb\n";
        assert_eq!(place("before marker # routes", text), lines_at(1, "  "));
        assert_eq!(place("after marker # routes", text), lines_at(2, "  "));
        assert_eq!(place("after first line matching ^b", text), lines_at(3, ""));
        assert_eq!(place("after last line matching ^b", text), lines_at(4, ""));
        assert_eq!(place("start of file", text), lines_at(0, ""));
        assert_eq!(place("end of file", text), lines_at(4, ""));
        assert_eq!(
            place("before marker # nope", text),
            Resolution::Missing {
                what: "line `# nope`".into()
            }
        );
        assert_eq!(
            resolve_in_text(&Anchor::parse("end of imports").unwrap(), text),
            None
        );
    }

    #[test]
    fn a_miss_is_said_with_the_phrase_and_the_file() {
        let anchor = Anchor::parse("end of parameters in CloseTable.__init__").unwrap();
        let missing = Resolution::Missing {
            what: "function `CloseTable.__init__`".into(),
        };
        assert_eq!(
            resolution_text(&anchor, "close_table.txt", &missing),
            "`end of parameters in CloseTable.__init__`: no function `CloseTable.__init__` in close_table.txt"
        );
        let several = Resolution::Several {
            what: "call `CloseTable`".into(),
            count: 2,
        };
        assert_eq!(
            resolution_text(&anchor, "x.txt", &several),
            "`end of parameters in CloseTable.__init__`: 2 places are call `CloseTable` in x.txt; narrow it with `in FUNCTION`"
        );
    }

    #[test]
    fn an_unknown_target_is_refused_with_the_list() {
        let error = Anchor::parse("end of cases in update").unwrap_err();
        assert!(
            error.starts_with(
                "`end of cases in update`: unknown target `cases in update` (targets: imports, "
            ),
            "{error}"
        );
    }

    #[test]
    fn a_phrase_without_a_position_is_refused() {
        assert_eq!(
            Anchor::parse("imports").unwrap_err(),
            "`imports`: a phrase starts with `start of`, `end of`, `before` or `after`"
        );
    }

    #[test]
    fn positions_a_target_lacks_are_refused() {
        assert_eq!(
            Anchor::parse("end of marker x").unwrap_err(),
            "`end of marker x`: a line has no inside: write `before` or `after`"
        );
        assert_eq!(
            Anchor::parse("after file").unwrap_err(),
            "`after file`: a file has no outside: write `start of file` or `end of file`"
        );
    }

    #[test]
    fn a_regex_that_does_not_compile_is_refused() {
        let error = Anchor::parse("after first line matching (").unwrap_err();
        assert!(
            error.starts_with("`after first line matching (`: the regex `(` does not compile"),
            "{error}"
        );
    }
}
