//! A rule: a convention that checks itself. Read from `.sorcerer/spellbook/rules.toml`, one
//! `[rules.<id>]` table each. A rule names the files it looks at, and either a `match` (the
//! short form) or a `query` (a tree-sitter query whose `@finding` captures are the findings).

use regex::Regex;
use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use crate::domain::fix::{Fix, Replace};
use crate::domain::generator_problems::unknown_key_text;
use crate::domain::path_pattern::PathPattern;

/// How serious a finding is: only an `error` makes `spellbook check` fail.
/// A rule that names no severity is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Hint,
    Warn,
    Error,
}

impl Severity {
    /// The word a person writes: `hint`, `warn` or `error`.
    pub fn word(self) -> &'static str {
        match self {
            Severity::Hint => "hint",
            Severity::Warn => "warn",
            Severity::Error => "error",
        }
    }
}

/// The short form of a rule: a node kind, a regular expression its text must match, one it must
/// not, or several forms of which any one is a finding.
///
/// Example: `{ kind = "import_statement", regex = "sqlite3" }` finds an import whose text has
/// `sqlite3`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchForm {
    pub kind: Option<String>,
    pub regex: Option<String>,
    /// A regular expression the node's text must not match.
    pub not: Option<String>,
    pub any: Vec<MatchForm>,
}

/// What a rule looks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleBody {
    Match(MatchForm),
    /// A tree-sitter query in the file's language, as the person wrote it.
    Query(String),
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub files: PathPattern,
    pub exclude: Vec<PathPattern>,
    pub body: RuleBody,
    pub message: String,
    pub severity: Severity,
    pub since: Option<String>,
    pub why: Option<String>,
    /// The generators that answer a finding, best first; the first is the preferred one.
    pub fixes: Vec<Fix>,
}

impl Rule {
    /// Whether the rule looks at a file (a path relative to the root, with `/`).
    pub fn applies_to(&self, path: &str) -> bool {
        self.files.matches(path).is_some()
            && !self
                .exclude
                .iter()
                .any(|pattern| pattern.matches(path).is_some())
    }
}

const RULE_KEYS: &[&str] = &[
    "files", "exclude", "match", "query", "message", "severity", "since", "why", "fix", "fixes",
];
const MATCH_KEYS: &[&str] = &["kind", "regex", "not", "any"];
const FIX_KEYS: &[&str] = &["generator", "label", "with", "replace"];
const REPLACE_KEYS: &[&str] = &["enclosing", "text"];
const LATER_RULE_KEYS: &[&str] = &["missing"];

/// The rules in the text of a `rules.toml`, in file order; each problem names the rule and the key.
///
/// Example: a rule with both `match` and `query` is "rule x: write `match` or `query`, not both".
pub fn parse_rules(text: &str) -> Result<Vec<Rule>, String> {
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("rules.toml: {}", e.message().trim()))?;
    let mut rules = Vec::new();
    for (key, value) in &table {
        if key != "rules" {
            return Err(format!(
                "rules.toml: {} (rules are `[rules.<id>]` tables)",
                unknown_key_text(key, &["rules"])
            ));
        }
        let Some(ruleset) = value.as_table() else {
            return Err("rules.toml: `rules` is a table of `[rules.<id>]`".into());
        };
        for (id, body) in ruleset {
            let rule = parse_rule(id, body).map_err(|why| format!("rule {id}: {why}"))?;
            rules.push(rule);
        }
    }
    Ok(rules)
}

fn parse_rule(id: &str, value: &toml::Value) -> Result<Rule, String> {
    let table = value.as_table().ok_or("it is a table, `[rules.<id>]`")?;
    refuse_unknown_keys(table, RULE_KEYS)?;
    let files = pattern_of(table, "files")?
        .ok_or("`files` is missing: the path pattern of the files it looks at")?;
    let message =
        string_of(table, "message")?.ok_or("`message` is missing: the sentence a finding says")?;
    let body = match (table.get("match"), table.get("query")) {
        (Some(_), Some(_)) => return Err("write `match` or `query`, not both".into()),
        (None, None) => {
            return Err("write `match` (the short form) or `query` (a tree-sitter query)".into());
        }
        (Some(form), None) => {
            RuleBody::Match(match_form(form).map_err(|why| format!("match: {why}"))?)
        }
        (None, Some(query)) => {
            RuleBody::Query(query.as_str().ok_or("`query` is a string")?.to_string())
        }
    };
    let fixes = fixes_of(table, &files)?;
    Ok(Rule {
        id: id.to_string(),
        files,
        exclude: patterns_of(table, "exclude")?,
        body,
        message,
        severity: severity_of(table)?,
        since: string_of(table, "since")?,
        why: string_of(table, "why")?,
        fixes,
    })
}

/// A key the table has that a rule does not: a later milestone's, or a slip.
fn refuse_unknown_keys(table: &toml::Table, known: &[&str]) -> Result<(), String> {
    for key in table.keys() {
        if LATER_RULE_KEYS.contains(&key.as_str()) {
            return Err(format!("`{key}` arrives in milestone 2b"));
        }
        if !known.contains(&key.as_str()) {
            return Err(unknown_key_text(key, known));
        }
    }
    Ok(())
}

/// The rule's fixes: `fix = "<generator>"` (one fix, labelled with the generator's name, whose
/// `with` passes each slot of `files` on as the input of that name) or `[[fixes]]` tables.
fn fixes_of(table: &toml::Table, files: &PathPattern) -> Result<Vec<Fix>, String> {
    match (table.get("fix"), table.get("fixes")) {
        (Some(_), Some(_)) => Err("write `fix` or `fixes`, not both".into()),
        (None, None) => Ok(Vec::new()),
        (Some(_), None) => {
            let generator = string_of(table, "fix")?.unwrap_or_default();
            Ok(vec![Fix {
                generator,
                label: None,
                with: files
                    .slots()
                    .iter()
                    .map(|slot| (slot.clone(), format!("{{= {slot} =}}")))
                    .collect(),
                replace: Vec::new(),
            }])
        }
        (None, Some(list)) => list
            .as_array()
            .ok_or("`fixes` is a list of tables, `[[rules.<id>.fixes]]`")?
            .iter()
            .enumerate()
            .map(|(index, item)| fix_of(item).map_err(|why| format!("fixes[{}]: {why}", index + 1)))
            .collect(),
    }
}

fn fix_of(value: &toml::Value) -> Result<Fix, String> {
    let table = value
        .as_table()
        .ok_or("it is a table, `{ generator, label, with, replace }`")?;
    refuse_unknown_keys(table, FIX_KEYS)?;
    let generator = string_of(table, "generator")?
        .ok_or("`generator` is missing: the generator that makes the fix")?;
    let with = match table.get("with") {
        None => BTreeMap::new(),
        Some(with) => with
            .as_table()
            .ok_or("`with` is a table of input = template")?
            .iter()
            .map(|(name, value)| {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("`with.{name}` is a string (a template)"))?;
                Ok((name.clone(), text.to_string()))
            })
            .collect::<Result<_, String>>()?,
    };
    let replace = match table.get("replace") {
        None => Vec::new(),
        Some(list) => list
            .as_array()
            .ok_or("`replace` is a list of `{ enclosing, text }`")?
            .iter()
            .map(replace_of)
            .collect::<Result<_, _>>()?,
    };
    Ok(Fix {
        generator,
        label: string_of(table, "label")?,
        with,
        replace,
    })
}

fn replace_of(value: &toml::Value) -> Result<Replace, String> {
    let table = value
        .as_table()
        .ok_or("each `replace` case is a table, `{ enclosing, text }`")?;
    refuse_unknown_keys(table, REPLACE_KEYS)?;
    let enclosing = match table.get("enclosing") {
        Some(toml::Value::String(kind)) => vec![kind.clone()],
        Some(toml::Value::Array(kinds)) => kinds
            .iter()
            .map(|kind| kind.as_str().map(String::from))
            .collect::<Option<Vec<_>>>()
            .ok_or("`enclosing` is a list of node kinds")?,
        Some(_) => return Err("`enclosing` is a list of node kinds".into()),
        None => {
            return Err("`enclosing` is missing: the node kinds whose lines it replaces".into());
        }
    };
    Ok(Replace {
        enclosing,
        text: string_of(table, "text")?
            .ok_or("`text` is missing: what the lines become (\"\" deletes them)")?,
    })
}

fn string_of(table: &toml::Table, key: &str) -> Result<Option<String>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(|text| Some(text.to_string()))
            .ok_or_else(|| format!("`{key}` is a string")),
    }
}

fn pattern_of(table: &toml::Table, key: &str) -> Result<Option<PathPattern>, String> {
    string_of(table, key)?
        .map(|text| PathPattern::parse(&text).map_err(|why| format!("{key}: {why}")))
        .transpose()
}

fn patterns_of(table: &toml::Table, key: &str) -> Result<Vec<PathPattern>, String> {
    let Some(value) = table.get(key) else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| format!("`{key}` is a list of path patterns"))?;
    list.iter()
        .map(|item| {
            let text = item
                .as_str()
                .ok_or_else(|| format!("`{key}` is a list of path patterns"))?;
            PathPattern::parse(text).map_err(|why| format!("{key}: {why}"))
        })
        .collect()
}

fn severity_of(table: &toml::Table) -> Result<Severity, String> {
    match string_of(table, "severity")?.as_deref() {
        None | Some("error") => Ok(Severity::Error),
        Some("warn") => Ok(Severity::Warn),
        Some("hint") => Ok(Severity::Hint),
        Some(other) => Err(format!(
            "severity = \"{other}\" is not one of: hint, warn, error"
        )),
    }
}

fn match_form(value: &toml::Value) -> Result<MatchForm, String> {
    let table = value
        .as_table()
        .ok_or("it is a table, `{ kind, regex, … }`")?;
    refuse_unknown_keys(table, MATCH_KEYS)?;
    let regex = checked_regex(string_of(table, "regex")?)?;
    let not = match table.get("not") {
        None => None,
        Some(inner) => {
            let inner = inner
                .as_table()
                .ok_or("`not` is a table, `{ regex = \"…\" }`")?;
            if inner.keys().any(|key| key != "regex") {
                return Err("`not` holds only `regex`".into());
            }
            checked_regex(string_of(inner, "regex")?)?
        }
    };
    let any = match table.get("any") {
        None => Vec::new(),
        Some(list) => list
            .as_array()
            .ok_or("`any` is a list of match forms")?
            .iter()
            .map(match_form)
            .collect::<Result<_, _>>()?,
    };
    let form = MatchForm {
        kind: string_of(table, "kind")?,
        regex,
        not,
        any,
    };
    if form.kind.is_none() && form.regex.is_none() && form.any.is_empty() {
        return Err("name a `kind`, a `regex` or an `any`".into());
    }
    Ok(form)
}

/// The expression, when it compiles (the query uses the same engine).
fn checked_regex(text: Option<String>) -> Result<Option<String>, String> {
    if let Some(text) = &text {
        Regex::new(text).map_err(|e| format!("the regex `{text}` does not compile ({e})"))?;
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"
[rules.commands-use-ports]
files    = "src/{app}/{service}/commands/*.txt"
exclude  = ["**/__init__.txt"]
match    = { any = [{ kind = "import_statement", regex = "sqlite3" }] }
message  = "Commands talk to ports"
severity = "error"
"#;

    #[test]
    fn a_rule_reads_with_its_keys() {
        let rules = parse_rules(ONE).unwrap();
        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        assert_eq!(rule.id, "commands-use-ports");
        assert_eq!(rule.severity, Severity::Error);
        assert!(rule.applies_to("src/bistro/booking/commands/a.txt"));
        assert!(!rule.applies_to("src/bistro/booking/commands/__init__.txt"));
        assert_eq!(
            rule.body,
            RuleBody::Match(MatchForm {
                any: vec![MatchForm {
                    kind: Some("import_statement".into()),
                    regex: Some("sqlite3".into()),
                    ..MatchForm::default()
                }],
                ..MatchForm::default()
            })
        );
    }

    #[test]
    fn severity_defaults_to_error() {
        let rules =
            parse_rules("[rules.a]\nfiles = \"x\"\nmessage = \"m\"\nmatch = { kind = \"k\" }\n")
                .unwrap();
        assert_eq!(rules[0].severity, Severity::Error);
    }

    #[test]
    fn missing_arrives_in_milestone_2b() {
        let error = parse_rules(&format!("{ONE}\nmissing = \"x\"\n")).unwrap_err();
        assert_eq!(
            error,
            "rule commands-use-ports: `missing` arrives in milestone 2b"
        );
    }

    #[test]
    fn the_fix_shorthand_passes_the_slots_on() {
        let rules = parse_rules(&format!("{ONE}fix = \"use-port\"\n")).unwrap();
        let fix = &rules[0].fixes[0];
        assert_eq!(fix.generator, "use-port");
        assert_eq!(fix.label, None);
        assert_eq!(fix.with.get("app").map(String::as_str), Some("{= app =}"));
        assert_eq!(
            fix.with.get("service").map(String::as_str),
            Some("{= service =}")
        );
        assert!(fix.replace.is_empty());
    }

    #[test]
    fn fixes_read_as_tables_best_first() {
        let text = format!(
            "{ONE}\n[[rules.commands-use-ports.fixes]]\ngenerator = \"use-port\"\nlabel = \"Use a port\"\nwith = {{ unit = \"{{= stem =}}\" }}\nreplace = [{{ enclosing = [\"import_statement\"], text = \"\" }}, {{ enclosing = \"call\", text = \"x\" }}]\n[[rules.commands-use-ports.fixes]]\ngenerator = \"add-port\"\n"
        );
        let rules = parse_rules(&text).unwrap();
        let fixes = &rules[0].fixes;
        assert_eq!(fixes.len(), 2);
        assert_eq!(fixes[0].label.as_deref(), Some("Use a port"));
        assert_eq!(
            fixes[0].with.get("unit").map(String::as_str),
            Some("{= stem =}")
        );
        assert_eq!(
            fixes[0].replace,
            vec![
                Replace {
                    enclosing: vec!["import_statement".into()],
                    text: String::new()
                },
                Replace {
                    enclosing: vec!["call".into()],
                    text: "x".into()
                },
            ]
        );
        assert_eq!(fixes[1].generator, "add-port");
    }

    #[test]
    fn fix_and_fixes_together_are_refused() {
        let text =
            format!("{ONE}fix = \"g\"\n[[rules.commands-use-ports.fixes]]\ngenerator = \"g\"\n");
        assert_eq!(
            parse_rules(&text).unwrap_err(),
            "rule commands-use-ports: write `fix` or `fixes`, not both"
        );
    }

    #[test]
    fn a_fix_slip_is_named() {
        let text = format!("{ONE}\n[[rules.commands-use-ports.fixes]]\ngenrator = \"g\"\n");
        assert_eq!(
            parse_rules(&text).unwrap_err(),
            "rule commands-use-ports: fixes[1]: `genrator` is not a key; did you mean `generator`?"
        );
    }

    #[test]
    fn match_and_query_together_are_refused() {
        let error = parse_rules("[rules.a]\nfiles = \"x\"\nmessage = \"m\"\nmatch = { kind = \"k\" }\nquery = \"(k) @finding\"\n").unwrap_err();
        assert_eq!(error, "rule a: write `match` or `query`, not both");
    }

    #[test]
    fn a_slip_is_named() {
        let error =
            parse_rules("[rules.a]\nfiles = \"x\"\nmesage = \"m\"\nmatch = { kind = \"k\" }\n")
                .unwrap_err();
        assert_eq!(
            error,
            "rule a: `mesage` is not a key; did you mean `message`?"
        );
        let severity = parse_rules("[rules.a]\nfiles = \"x\"\nmessage = \"m\"\nseverity = \"bad\"\nmatch = { kind = \"k\" }\n").unwrap_err();
        assert!(
            severity.contains("is not one of: hint, warn, error"),
            "{severity}"
        );
    }

    #[test]
    fn a_regex_that_does_not_compile_is_refused() {
        let error = parse_rules(
            "[rules.a]\nfiles = \"x\"\nmessage = \"m\"\nmatch = { kind = \"k\", regex = \"(\" }\n",
        )
        .unwrap_err();
        assert!(
            error.starts_with("rule a: match: the regex `(` does not compile"),
            "{error}"
        );
    }
}
