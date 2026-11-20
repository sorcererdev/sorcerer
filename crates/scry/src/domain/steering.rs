//! Steering: what the reviewer is told about the project, read from
//! `.sorcerer/scry.toml`. The file has three keys, all optional:
//!
//! - `context`: prose about the project;
//! - `include`: a list of files whose text is read in as more prose, in order;
//! - `[[rule]]`: the project's rules, each an `id` and its `text`.
//!   A finding cites a rule by its id.
//!
//! The file is checked strictly: an unknown key, a malformed or repeated
//! id, or an empty text is an error naming the file and the key.

use std::path::{Path, PathBuf};

use sorcerer_core::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Include {
    pub path: PathBuf,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Steering {
    pub context: String,
    pub includes: Vec<Include>,
    pub rules: Vec<Rule>,
}

/// The file as written, before the files it includes are read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawSteering {
    pub context: String,
    pub include: Vec<PathBuf>,
    pub rules: Vec<Rule>,
}

/// The most steering a review call carries, in bytes: 32 KB, about 8,000
/// tokens.
pub const STEERING_CAP: usize = 32 * 1024;

/// The steering file's text, checked and read. It may hold only `context`
/// (a string), `include` (a list of paths, none empty) and `[[rule]]`
/// tables (each with only `id` and `text`; every id well formed and used
/// once, no text empty). Anything else is an error naming `file`.
///
/// Example: `[[rule]]` with `id = "Log_Text"` is refused: an id is
/// lowercase letters, digits and hyphens.
pub fn parse_steering(text: &str, file: &Path) -> Result<RawSteering> {
    let file = SteeringFile(file);
    let table: toml::Table = toml::from_str(text).map_err(|e| file.error(e.message()))?;
    let mut raw = RawSteering::default();
    for (key, value) in &table {
        match key.as_str() {
            "context" => raw.context = file.context(value)?,
            "include" => raw.include = file.includes(value)?,
            "rule" => raw.rules = file.rules(value)?,
            other => return Err(file.error(format!("unknown key `{other}`"))),
        }
    }
    Ok(raw)
}

/// The steering file being read, so each check's error can name it.
struct SteeringFile<'a>(&'a Path);

impl SteeringFile<'_> {
    fn error(&self, why: impl Into<String>) -> Error {
        Error::Config {
            file: self.0.to_path_buf(),
            why: why.into(),
        }
    }

    fn context(&self, value: &toml::Value) -> Result<String> {
        let context = value
            .as_str()
            .ok_or_else(|| self.error("`context` is not a string"))?;
        Ok(context.to_string())
    }

    /// The include paths, each a string that is not blank. Errors count
    /// from 1: `include[2] is empty`.
    fn includes(&self, value: &toml::Value) -> Result<Vec<PathBuf>> {
        let items = value
            .as_array()
            .ok_or_else(|| self.error("`include` is not a list of paths"))?;
        let include_at = |(i, item): (usize, &toml::Value)| {
            let n = i + 1;
            let path = item
                .as_str()
                .ok_or_else(|| self.error(format!("include[{n}] is not a path")))?;
            if path.trim().is_empty() {
                return Err(self.error(format!("include[{n}] is empty")));
            }
            Ok(PathBuf::from(path))
        };
        items.iter().enumerate().map(include_at).collect()
    }

    /// The rules in the order written. Each is checked against the ones
    /// before it, so a repeated id names the rule that had it first.
    fn rules(&self, value: &toml::Value) -> Result<Vec<Rule>> {
        let items = value
            .as_array()
            .ok_or_else(|| self.error("`rule` is not a list of [[rule]] tables"))?;
        let mut rules = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let rule = self.rule(i + 1, item, &rules)?;
            rules.push(rule);
        }
        Ok(rules)
    }

    /// The `n`th `[[rule]]` table (counting from 1), checked: only `id` and
    /// `text`, an id that is well formed and not one of `earlier`'s, and a
    /// text that is not blank.
    fn rule(&self, n: usize, item: &toml::Value, earlier: &[Rule]) -> Result<Rule> {
        let table = item
            .as_table()
            .ok_or_else(|| self.error(format!("[[rule]] {n} is not a table")))?;
        if let Some(key) = table.keys().find(|k| *k != "id" && *k != "text") {
            return Err(self.error(format!("unknown key `{key}` in [[rule]] {n}")));
        }
        let id = self.rule_id(n, table, earlier)?;
        let text = match table.get("text") {
            None => "",
            Some(v) => v.as_str().ok_or_else(|| {
                self.error(format!("[[rule]] {n} (`{id}`): text is not a string"))
            })?,
        };
        if text.trim().is_empty() {
            return Err(self.error(format!("[[rule]] {n} (`{id}`): text is empty")));
        }
        Ok(Rule {
            id: id.to_string(),
            text: text.to_string(),
        })
    }

    fn rule_id<'t>(&self, n: usize, table: &'t toml::Table, earlier: &[Rule]) -> Result<&'t str> {
        let id = match table.get("id") {
            None => return Err(self.error(format!("[[rule]] {n}: no id"))),
            Some(v) => v
                .as_str()
                .ok_or_else(|| self.error(format!("[[rule]] {n}: id is not a string")))?,
        };
        if !is_id(id) {
            return Err(self.error(format!(
                "[[rule]] {n}: id `{id}` is not an id (lowercase letters, digits and hyphens, starting with a letter, at most 40)"
            )));
        }
        if let Some(first) = earlier.iter().position(|r| r.id == id) {
            return Err(self.error(format!(
                "[[rule]] {n}: id `{id}` is also [[rule]] {}'s",
                first + 1
            )));
        }
        Ok(id)
    }
}

/// Whether `id` is a well-formed rule id: lowercase letters, digits and
/// single hyphens, starting with a letter, not ending with a hyphen, at
/// most 40 characters. `log-text-apart` is one; `Log-Text`, `log--text`
/// and `2-rules` are not.
fn is_id(id: &str) -> bool {
    id.len() <= 40
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// One part of the steering as the reviewer reads it, with a name for
/// telling the user which part is too big.
struct SteeringPart {
    name: String,
    text: String,
}

impl Steering {
    /// The rule with exactly this id.
    pub fn rule(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.id == id)
    }

    /// The id of the rule a finding cites, if the project has it. The
    /// reviewer may wrap the id or change its case, so it is trimmed, one
    /// pair of quotes, backticks or brackets is taken off, and it is
    /// lowercased; then it must match an id exactly. A rule is never found
    /// by part of its id or by its text.
    ///
    /// Example: `` `Log-Text-Apart` `` and `[log-text-apart]` both find
    /// `log-text-apart`; `log-text` finds nothing.
    pub fn lookup(&self, cited: &str) -> Option<&str> {
        let trimmed = cited.trim();
        let unwrapped = [('"', '"'), ('\'', '\''), ('`', '`'), ('[', ']')]
            .iter()
            .find_map(|(open, close)| trimmed.strip_prefix(*open)?.strip_suffix(*close))
            .unwrap_or(trimmed);
        let id = unwrapped.trim().to_lowercase();
        self.rule(&id).map(|r| r.id.as_str())
    }

    /// The steering as the review prompt carries it: the context, each
    /// included file, then the rules, a blank line apart. With none of
    /// them it is `(none)`.
    pub fn steering_text(&self) -> String {
        let parts: Vec<String> = self.parts().into_iter().map(|part| part.text).collect();
        if parts.is_empty() {
            "(none)".into()
        } else {
            parts.join("\n\n")
        }
    }

    /// The parts of the steering text, named `context`, `include <path>`
    /// and `the rules`. An empty context or no rules leaves its part out.
    fn parts(&self) -> Vec<SteeringPart> {
        let context = self.context.trim();
        let context_part = (!context.is_empty()).then(|| SteeringPart {
            name: "context".into(),
            text: context.into(),
        });
        let include_parts = self.includes.iter().map(|include| SteeringPart {
            name: format!("include {}", include.path.display()),
            text: format!(
                "<include path=\"{}\">\n{}\n</include>",
                include.path.display(),
                include.text.trim()
            ),
        });
        let rules_part = (!self.rules.is_empty()).then(|| SteeringPart {
            name: "the rules".into(),
            text: format!("Rules (cite one by its id):\n{}", self.rules_text()),
        });
        context_part
            .into_iter()
            .chain(include_parts)
            .chain(rules_part)
            .collect()
    }

    /// The rules as the reviewer and Jev see them, one list item each;
    /// `(none)` when there are none.
    ///
    /// Example: a rule `log-text-apart` whose text is two lines reads
    /// ```text
    /// - log-text-apart: Run-log text lives in its own functions,
    ///   so the logic reads without it.
    /// ```
    pub fn rules_text(&self) -> String {
        if self.rules.is_empty() {
            return "(none)".into();
        }
        self.rules
            .iter()
            .map(rule_item)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The steering text's size in bytes.
    pub fn size(&self) -> usize {
        self.steering_text().len()
    }

    /// When the steering text is over 32 KB, why it cannot be sent: its
    /// size, the cap and its largest part, so the user knows what to cut.
    /// None when it fits.
    pub fn over_cap(&self) -> Option<String> {
        let size = self.size();
        if size <= STEERING_CAP {
            return None;
        }
        let largest = self
            .parts()
            .into_iter()
            .max_by_key(|part| part.text.len())?;
        Some(format!(
            "the steering is {size} bytes, over the cap of {STEERING_CAP} (32 KB); its largest part is {} ({} bytes)",
            largest.name,
            largest.text.len()
        ))
    }
}

/// A rule as a list item: `- id: first line`, each further line indented
/// two spaces, blank lines kept blank.
fn rule_item(rule: &Rule) -> String {
    let mut lines = rule.text.trim().lines();
    let first = format!("- {}: {}", rule.id, lines.next().unwrap_or(""));
    let rest = lines.map(|line| {
        if line.trim().is_empty() {
            String::new()
        } else {
            format!("  {line}")
        }
    });
    std::iter::once(first)
        .chain(rest)
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> PathBuf {
        PathBuf::from(".sorcerer/scry.toml")
    }

    fn parsed(text: &str) -> RawSteering {
        parse_steering(text, &file()).unwrap()
    }

    fn refused(text: &str) -> String {
        parse_steering(text, &file()).unwrap_err().to_string()
    }

    fn steering(raw: RawSteering, includes: Vec<Include>) -> Steering {
        Steering {
            context: raw.context,
            includes,
            rules: raw.rules,
        }
    }

    const FULL: &str = r#"
context = "Bistro is ports and adapters."
include = ["docs/conventions.md"]

[[rule]]
id = "ports"
text = """
Commands go through ports (must)
A command never touches the database."""

[[rule]]
id = "clock"
text = "Time comes from the clock port."
"#;

    #[test]
    fn a_file_with_context_includes_and_rules_parses() {
        let raw = parsed(FULL);
        assert_eq!(raw.context, "Bistro is ports and adapters.");
        assert_eq!(raw.include, [PathBuf::from("docs/conventions.md")]);
        let project_steering = steering(raw, vec![]);
        let ids: Vec<&str> = project_steering
            .rules
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(ids, ["ports", "clock"]);
        assert_eq!(
            project_steering.rule("ports").unwrap().text,
            "Commands go through ports (must)\nA command never touches the database."
        );
        assert_eq!(project_steering.lookup("`Ports`"), Some("ports"));
        assert_eq!(project_steering.lookup(" \"clock\" "), Some("clock"));
        assert_eq!(project_steering.lookup("[ports]"), Some("ports"));
        assert_eq!(project_steering.lookup("ports-x"), None);
        assert_eq!(project_steering.lookup("Commands go through ports"), None);
        assert_eq!(project_steering.lookup(""), None);
        assert!(project_steering.rule("Ports").is_none());
    }

    #[test]
    fn unknown_keys_are_refused_naming_them() {
        let e = refused("level = 1\n");
        assert!(e.contains("unknown key `level`"), "{e}");
        assert!(e.contains(".sorcerer/scry.toml"), "{e}");
        let e = refused(
            "[[rule]]\nid = \"a\"\ntext = \"t\"\n\n[[rule]]\nid = \"b\"\ntext = \"t\"\nlevel = \"must\"\n",
        );
        assert!(e.contains("unknown key `level` in [[rule]] 2"), "{e}");
    }

    #[test]
    fn a_malformed_id_is_refused() {
        let long = "a".repeat(41);
        for id in [
            "Ports",
            "web_commands",
            "-a",
            "a--b",
            "a-",
            "1a",
            long.as_str(),
        ] {
            let e = refused(&format!("[[rule]]\nid = \"{id}\"\ntext = \"t\"\n"));
            assert!(
                e.contains(&format!("[[rule]] 1: id `{id}` is not an id")),
                "{id}: {e}"
            );
        }
        let forty = format!("[[rule]]\nid = \"{}\"\ntext = \"t\"\n", "a".repeat(40));
        assert_eq!(parsed(&forty).rules.len(), 1);
    }

    #[test]
    fn a_duplicate_id_is_refused_naming_both() {
        let e = refused(
            "[[rule]]\nid = \"ports\"\ntext = \"a\"\n\n[[rule]]\nid = \"ports\"\ntext = \"b\"\n",
        );
        assert!(
            e.contains("[[rule]] 2: id `ports` is also [[rule]] 1's"),
            "{e}"
        );
    }

    #[test]
    fn an_empty_text_or_include_is_refused() {
        let e = refused("[[rule]]\nid = \"clock\"\ntext = \"  \"\n");
        assert!(e.contains("[[rule]] 1 (`clock`): text is empty"), "{e}");
        let e = refused("[[rule]]\nid = \"clock\"\n");
        assert!(e.contains("[[rule]] 1 (`clock`): text is empty"), "{e}");
        let e = refused("include = [\"a.md\", \" \"]\n");
        assert!(e.contains("include[2] is empty"), "{e}");
    }

    #[test]
    fn empty_steering_renders_none() {
        let project_steering = Steering::default();
        assert_eq!(project_steering.steering_text(), "(none)");
        assert_eq!(project_steering.rules_text(), "(none)");
        assert_eq!(steering(parsed(""), vec![]), Steering::default());
    }

    #[test]
    fn steering_text_renders_context_includes_then_rules() {
        let project_steering = steering(
            parsed(FULL),
            vec![Include {
                path: "docs/conventions.md".into(),
                text: "A command never panics.\n".into(),
            }],
        );
        assert_eq!(
            project_steering.steering_text(),
            "Bistro is ports and adapters.\n\n\
             <include path=\"docs/conventions.md\">\nA command never panics.\n</include>\n\n\
             Rules (cite one by its id):\n\
             - ports: Commands go through ports (must)\n  A command never touches the database.\n\
             - clock: Time comes from the clock port."
        );
        assert_eq!(
            project_steering.size(),
            project_steering.steering_text().len()
        );
        let blank = Steering {
            rules: vec![Rule {
                id: "a".into(),
                text: "one\n\nthree".into(),
            }],
            ..Steering::default()
        };
        assert_eq!(blank.rules_text(), "- a: one\n\n  three");
    }

    #[test]
    fn over_cap_names_the_largest_part() {
        let project_steering = Steering {
            context: "c".into(),
            includes: vec![Include {
                path: "docs/big.md".into(),
                text: "x".repeat(STEERING_CAP),
            }],
            rules: vec![],
        };
        let why = project_steering.over_cap().unwrap();
        assert!(why.contains(&project_steering.size().to_string()), "{why}");
        assert!(why.contains("32768"), "{why}");
        assert!(why.contains("include docs/big.md"), "{why}");
        assert!(steering(parsed(FULL), vec![]).over_cap().is_none());
    }
}
