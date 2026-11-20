//! Steering: what the reviewer is told about the project, from
//! `.sorcerer/scry.toml`: `context` (prose), `include` (files read in as
//! more prose, in order) and `[[rule]]`s, each an `id` and its `text`. A
//! finding cites a rule by its id. Validation is strict and loud: an
//! unknown key, a malformed or repeated id, or an empty text is an error
//! naming the file and the key.

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

/// The file as written, before its includes are read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawSteering {
    pub context: String,
    pub include: Vec<PathBuf>,
    pub rules: Vec<Rule>,
}

/// The most rendered steering a review call carries: about 8k tokens.
pub const STEERING_CAP: usize = 32 * 1024;

/// TEXT, the contents of FILE, checked: only `context`, `include` and
/// `[[rule]]` (with only `id` and `text`); every id well formed and
/// unique; no text and no include empty.
pub fn parse_steering(text: &str, file: &Path) -> Result<RawSteering> {
    let bad = |why: String| Error::Config {
        file: file.to_path_buf(),
        why,
    };
    let table: toml::Table = toml::from_str(text).map_err(|e| bad(e.message().to_string()))?;
    let mut raw = RawSteering::default();
    for (key, value) in &table {
        match key.as_str() {
            "context" => {
                raw.context = value
                    .as_str()
                    .ok_or_else(|| bad("`context` is not a string".into()))?
                    .to_string();
            }
            "include" => {
                let items = value
                    .as_array()
                    .ok_or_else(|| bad("`include` is not a list of paths".into()))?;
                for (i, item) in items.iter().enumerate() {
                    let path = item
                        .as_str()
                        .ok_or_else(|| bad(format!("include[{}] is not a path", i + 1)))?;
                    if path.trim().is_empty() {
                        return Err(bad(format!("include[{}] is empty", i + 1)));
                    }
                    raw.include.push(PathBuf::from(path));
                }
            }
            "rule" => {
                let items = value
                    .as_array()
                    .ok_or_else(|| bad("`rule` is not a list of [[rule]] tables".into()))?;
                for (i, item) in items.iter().enumerate() {
                    let n = i + 1;
                    let t = item
                        .as_table()
                        .ok_or_else(|| bad(format!("[[rule]] {n} is not a table")))?;
                    if let Some(k) = t.keys().find(|k| *k != "id" && *k != "text") {
                        return Err(bad(format!("unknown key `{k}` in [[rule]] {n}")));
                    }
                    let id = match t.get("id") {
                        None => return Err(bad(format!("[[rule]] {n}: no id"))),
                        Some(v) => v
                            .as_str()
                            .ok_or_else(|| bad(format!("[[rule]] {n}: id is not a string")))?,
                    };
                    if !is_id(id) {
                        return Err(bad(format!(
                            "[[rule]] {n}: id `{id}` is not an id (lowercase letters, digits and hyphens, starting with a letter, at most 40)"
                        )));
                    }
                    if let Some(first) = raw.rules.iter().position(|r| r.id == id) {
                        return Err(bad(format!(
                            "[[rule]] {n}: id `{id}` is also [[rule]] {}'s",
                            first + 1
                        )));
                    }
                    let text = match t.get("text") {
                        None => "",
                        Some(v) => v.as_str().ok_or_else(|| {
                            bad(format!("[[rule]] {n} (`{id}`): text is not a string"))
                        })?,
                    };
                    if text.trim().is_empty() {
                        return Err(bad(format!("[[rule]] {n} (`{id}`): text is empty")));
                    }
                    raw.rules.push(Rule {
                        id: id.to_string(),
                        text: text.to_string(),
                    });
                }
            }
            other => return Err(bad(format!("unknown key `{other}`"))),
        }
    }
    Ok(raw)
}

/// Lowercase letters, digits and single hyphens, starting with a letter,
/// not ending with a hyphen, at most 40 characters.
fn is_id(id: &str) -> bool {
    id.len() <= 40
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl Steering {
    /// The rule whose id is ID exactly.
    pub fn rule(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.id == id)
    }

    /// The id a finding's CITED rule names: trimmed, one pair of quotes,
    /// backticks or brackets taken off, lowercased, then an exact id. No
    /// substring and no text matching.
    pub fn lookup(&self, cited: &str) -> Option<&str> {
        let c = cited.trim();
        let c = [('"', '"'), ('\'', '\''), ('`', '`'), ('[', ']')]
            .iter()
            .find_map(|(a, z)| c.strip_prefix(*a).and_then(|r| r.strip_suffix(*z)))
            .unwrap_or(c)
            .trim()
            .to_lowercase();
        self.rule(&c).map(|r| r.id.as_str())
    }

    /// The prompt's steering block: the context, each include, then the
    /// rules, a blank line apart; `(none)` when there is nothing.
    pub fn steering_text(&self) -> String {
        let parts: Vec<String> = self.parts().into_iter().map(|(_, t)| t).collect();
        if parts.is_empty() {
            "(none)".into()
        } else {
            parts.join("\n\n")
        }
    }

    /// The rendered parts, each named: `context`, `include PATH`, `the rules`.
    fn parts(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let context = self.context.trim();
        if !context.is_empty() {
            out.push(("context".to_string(), context.to_string()));
        }
        for i in &self.includes {
            out.push((
                format!("include {}", i.path.display()),
                format!(
                    "<include path=\"{}\">\n{}\n</include>",
                    i.path.display(),
                    i.text.trim()
                ),
            ));
        }
        if !self.rules.is_empty() {
            out.push((
                "the rules".to_string(),
                format!("Rules (cite one by its id):\n{}", self.rules_text()),
            ));
        }
        out
    }

    /// The rules as the prompt and Jev see them: `- id: first line`, further
    /// lines indented two spaces; `(none)` without rules.
    pub fn rules_text(&self) -> String {
        if self.rules.is_empty() {
            return "(none)".into();
        }
        self.rules
            .iter()
            .map(|r| {
                let mut lines = r.text.trim().lines();
                let mut s = format!("- {}: {}", r.id, lines.next().unwrap_or(""));
                for l in lines {
                    s.push('\n');
                    if !l.trim().is_empty() {
                        s.push_str("  ");
                        s.push_str(l);
                    }
                }
                s
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The rendered steering's size in bytes.
    pub fn size(&self) -> usize {
        self.steering_text().len()
    }

    /// Why the steering is too big to send, naming its size, the cap and its
    /// largest part; None when it fits.
    pub fn over_cap(&self) -> Option<String> {
        let size = self.size();
        if size <= STEERING_CAP {
            return None;
        }
        let (name, text) = self.parts().into_iter().max_by_key(|(_, t)| t.len())?;
        Some(format!(
            "the steering is {size} bytes, over the cap of {STEERING_CAP} (32 KB); its largest part is {name} ({} bytes)",
            text.len()
        ))
    }
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
