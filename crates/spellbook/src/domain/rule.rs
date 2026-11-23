//! A rule: a convention that checks itself. Read from `.sorcerer/spellbook/rules.toml`, one
//! `[rules.<id>]` table each. A rule names the files it looks at, and either a `match` (the
//! short form) or a `query` (a tree-sitter query whose `@finding` captures are the findings).

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::domain::generator_problems::unknown_key_text;
use crate::domain::path_pattern::PathPattern;

/// How serious a finding is: only an `error` makes `spellbook check` fail.
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
    "files", "exclude", "match", "query", "message", "severity", "since", "why",
];
const MATCH_KEYS: &[&str] = &["kind", "regex", "not", "any"];
const LATER_RULE_KEYS: &[&str] = &["missing", "fix", "fixes"];

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
    Ok(Rule {
        id: id.to_string(),
        files,
        exclude: patterns_of(table, "exclude")?,
        body,
        message,
        severity: severity_of(table)?,
        since: string_of(table, "since")?,
        why: string_of(table, "why")?,
    })
}

/// A key the table has that a rule does not: a later milestone's, or a slip.
fn refuse_unknown_keys(table: &toml::Table, known: &[&str]) -> Result<(), String> {
    for key in table.keys() {
        if LATER_RULE_KEYS.contains(&key.as_str()) {
            return Err(format!("`{key}` arrives in milestone 2"));
        }
        if !known.contains(&key.as_str()) {
            return Err(unknown_key_text(key, known));
        }
    }
    Ok(())
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
        None | Some("warn") => Ok(Severity::Warn),
        Some("hint") => Ok(Severity::Hint),
        Some("error") => Ok(Severity::Error),
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
files    = "src/{app}/{service}/commands/*.py"
exclude  = ["**/__init__.py"]
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
        assert!(rule.applies_to("src/bistro/booking/commands/a.py"));
        assert!(!rule.applies_to("src/bistro/booking/commands/__init__.py"));
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
    fn severity_defaults_to_warn() {
        let rules =
            parse_rules("[rules.a]\nfiles = \"x\"\nmessage = \"m\"\nmatch = { kind = \"k\" }\n")
                .unwrap();
        assert_eq!(rules[0].severity, Severity::Warn);
    }

    #[test]
    fn missing_arrives_in_milestone_2() {
        let error = parse_rules(&format!("{ONE}\nmissing = \"x\"\n")).unwrap_err();
        assert_eq!(
            error,
            "rule commands-use-ports: `missing` arrives in milestone 2"
        );
        let fixes = parse_rules("[rules.a]\nfiles = \"x\"\nmessage = \"m\"\nmatch = { kind = \"k\" }\n[[rules.a.fixes]]\ngenerator = \"g\"\n").unwrap_err();
        assert_eq!(fixes, "rule a: `fixes` arrives in milestone 2");
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
