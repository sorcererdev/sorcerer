//! A rule's fixes: the generators that answer a finding, best first. A fix names a generator,
//! fills some of its inputs from where the finding is (`with`), and says what becomes of the
//! found code (`replace`). Written in `rules.toml` as `fix = "<generator>"` (the shorthand) or as
//! `[[rules.<id>.fixes]]` tables.

use std::collections::BTreeMap;

use sorcerer_core::domain::templates::{Piece, fill, pieces};

use crate::domain::inputs::Given;

/// One fix of a rule.
///
/// Example: `{ generator = "use-port", label = "Use a port the service has", with = { unit =
/// "{= stem =}" } }` offers the `use-port` generator with `unit` already the file's stem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// The id of a generator of the project.
    pub generator: String,
    /// What the editor and `check` show, a template over the same names as `with`. `None` for the
    /// shorthand, whose label is the generator's `name`.
    pub label: Option<String>,
    /// The generator's inputs this fix gives, each a template over the [`FixBuiltins`].
    pub with: BTreeMap<String, String>,
    /// What becomes of the found code: the first case whose node kind encloses the finding wins.
    pub replace: Vec<Replace>,
}

/// One case of a fix's `replace`: when one of these node kinds encloses the finding (the found
/// node itself counts), that node's whole lines become `text`. An empty `text` deletes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replace {
    pub enclosing: Vec<String>,
    pub text: String,
}

/// The names a fix's `with` and `label` may use, and their values for one finding: the slots the
/// rule's `files` pattern bound on the path, `dir` (the file's folder), `stem` (its name without
/// extension), `file` (its path) and `finding` (the found node's text on one line).
///
/// Example: for `src/bistro/booking/commands/close_table.py` under
/// `src/{app}/{service}/{kind}/*.py`, `stem` is `close_table` and `service` is `booking`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixBuiltins {
    values: BTreeMap<String, String>,
}

/// The names every fix may use besides the rule's slots.
pub const BUILTIN_NAMES: &[&str] = &["dir", "stem", "file", "finding"];

impl FixBuiltins {
    /// The values for a finding in `file` (relative to the root, with `/`), the `slots` its rule
    /// bound and the found node's `finding` text.
    pub fn of(file: &str, slots: &BTreeMap<String, String>, finding: &str) -> FixBuiltins {
        let (dir, name) = file.rsplit_once('/').unwrap_or(("", file));
        let stem = name.split_once('.').map_or(name, |(stem, _)| stem);
        let mut values = slots.clone();
        values.insert("dir".into(), dir.into());
        values.insert("stem".into(), stem.into());
        values.insert("file".into(), file.into());
        values.insert("finding".into(), one_line(finding));
        FixBuiltins { values }
    }

    fn pairs(&self) -> Vec<(&str, &str)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect()
    }
}

/// The inputs a fix gives its generator for one finding: each `with` template filled.
///
/// Example: `with = { unit = "{= stem =}" }` on `close_table.py` gives `unit=close_table`.
pub fn fix_inputs(fix: &Fix, builtins: &FixBuiltins) -> Result<Given, String> {
    let pairs = builtins.pairs();
    fix.with
        .iter()
        .map(|(name, template)| Ok((name.clone(), fill(template, &pairs)?)))
        .collect::<Result<_, String>>()
        .map(Given)
}

/// The label a fix shows for one finding: its own, filled, or the generator's `name` for the
/// shorthand.
pub fn fix_label(
    fix: &Fix,
    generator_name: &str,
    builtins: &FixBuiltins,
) -> Result<String, String> {
    match &fix.label {
        Some(template) => fill(template, &builtins.pairs()),
        None => Ok(generator_name.to_string()),
    }
}

/// The found node's text as it may sit in a hole's instruction, which a placeholder puts inside
/// a string: on one line, without quotes or backslashes, at most 60 characters.
///
/// Example: `conn = sqlite3.connect(\n    "bistro.db")` becomes `conn = sqlite3.connect( bistro.db)`.
pub fn finding_for_instruction(text: &str) -> String {
    one_line(text)
        .chars()
        .filter(|c| !matches!(c, '"' | '\'' | '\\'))
        .take(60)
        .collect()
}

/// The text with every run of whitespace (line breaks included) made one space, trimmed.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the project has under a fix's generator id, as [`check_fix`] needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixGenerator {
    /// No generator by that id.
    Absent,
    /// A generator that does not load: its own problem, said where generators are listed.
    Broken,
    /// A generator that loads, with its inputs' names.
    Takes(Vec<String>),
}

/// Why a fix cannot be offered, said at load: a generator the project lacks, a `with` input the
/// generator does not have, or a `with`/`label` template over a name that is neither one of the
/// rule's slots nor a built-in. `generator_of` looks a generator up by id; a broken one is not
/// the fix's problem, so its inputs are not checked.
///
/// Example: `with = { port = "{= portt =}" }` gives "fix use-port: `with` uses `portt`, which is
/// not a slot of `files` or one of dir, stem, file, finding".
pub fn check_fix(
    fix: &Fix,
    slots: &[String],
    generator_of: &dyn Fn(&str) -> FixGenerator,
) -> Result<(), String> {
    let at = format!("fix {}", fix.generator);
    let inputs = match generator_of(&fix.generator) {
        FixGenerator::Absent => {
            return Err(format!(
                "{at}: the project has no generator `{}`",
                fix.generator
            ));
        }
        FixGenerator::Broken => None,
        FixGenerator::Takes(inputs) => Some(inputs),
    };
    for name in fix.with.keys() {
        if let Some(inputs) = inputs.as_ref().filter(|inputs| !inputs.contains(name)) {
            return Err(format!(
                "{at}: `with` gives `{name}`, an input the generator does not have (its inputs: {})",
                inputs.join(", ")
            ));
        }
    }
    let templates = fix.with.values().chain(fix.label.iter());
    for template in templates {
        for name in names_used(template).map_err(|why| format!("{at}: {why}"))? {
            if !slots.contains(&name) && !BUILTIN_NAMES.contains(&name.as_str()) {
                return Err(format!(
                    "{at}: `with` uses `{name}`, which is not a slot of `files` or one of {}",
                    BUILTIN_NAMES.join(", ")
                ));
            }
        }
    }
    Ok(())
}

/// The value names a template uses.
fn names_used(template: &str) -> Result<Vec<String>, String> {
    Ok(pieces(template)?
        .into_iter()
        .filter_map(|piece| match piece {
            Piece::Value { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots() -> BTreeMap<String, String> {
        [
            ("app", "bistro"),
            ("service", "booking"),
            ("kind", "commands"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
    }

    fn fix(with: &[(&str, &str)]) -> Fix {
        Fix {
            generator: "use-port".into(),
            label: None,
            with: with
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            replace: Vec::new(),
        }
    }

    #[test]
    fn with_fills_from_the_slots_and_the_builtins() {
        let builtins = FixBuiltins::of(
            "src/bistro/booking/commands/close_table.txt",
            &slots(),
            "import  sqlite3\n",
        );
        let given = fix_inputs(
            &fix(&[
                ("service", "{= service =}"),
                ("unit", "{= stem =}"),
                ("where", "{= dir =}"),
                ("what", "{= finding =}"),
            ]),
            &builtins,
        )
        .unwrap();
        assert_eq!(given.get("service"), Some("booking"));
        assert_eq!(given.get("unit"), Some("close_table"));
        assert_eq!(given.get("where"), Some("src/bistro/booking/commands"));
        assert_eq!(given.get("what"), Some("import sqlite3"));
    }

    #[test]
    fn the_shorthand_label_is_the_generator_name() {
        let builtins = FixBuiltins::of("a/b.txt", &slots(), "x");
        assert_eq!(
            fix_label(&fix(&[]), "Use a port", &builtins).unwrap(),
            "Use a port"
        );
        let mut labelled = fix(&[]);
        labelled.label = Some("Use a port in {= stem =}".into());
        assert_eq!(
            fix_label(&labelled, "Use a port", &builtins).unwrap(),
            "Use a port in b"
        );
    }

    #[test]
    fn a_finding_in_an_instruction_is_squashed() {
        assert_eq!(
            finding_for_instruction("conn = sqlite3.connect(\n    \"bistro.db\")"),
            "conn = sqlite3.connect( bistro.db)"
        );
        assert_eq!(finding_for_instruction("a\\b 'c'"), "ab c");
        assert_eq!(finding_for_instruction(&"x".repeat(80)).len(), 60);
    }

    fn generator_of(id: &str) -> FixGenerator {
        match id {
            "use-port" => FixGenerator::Takes(vec!["service".into(), "unit".into(), "port".into()]),
            "broken" => FixGenerator::Broken,
            _ => FixGenerator::Absent,
        }
    }

    #[test]
    fn a_broken_generator_leaves_its_inputs_unchecked() {
        let mut other = fix(&[("anything", "x")]);
        other.generator = "broken".into();
        check_fix(&other, &[], &generator_of).unwrap();
    }

    #[test]
    fn a_fix_over_known_names_passes() {
        let slot_names: Vec<String> = slots().into_keys().collect();
        check_fix(
            &fix(&[("service", "{= service =}"), ("unit", "{= stem|snake =}")]),
            &slot_names,
            &generator_of,
        )
        .unwrap();
    }

    #[test]
    fn a_fix_naming_no_generator_is_refused() {
        let mut other = fix(&[]);
        other.generator = "use-prot".into();
        assert_eq!(
            check_fix(&other, &[], &generator_of).unwrap_err(),
            "fix use-prot: the project has no generator `use-prot`"
        );
    }

    #[test]
    fn a_with_input_the_generator_lacks_is_refused() {
        assert_eq!(
            check_fix(&fix(&[("unitt", "x")]), &[], &generator_of).unwrap_err(),
            "fix use-port: `with` gives `unitt`, an input the generator does not have (its inputs: service, unit, port)"
        );
    }

    #[test]
    fn a_with_template_over_an_unknown_name_is_refused() {
        assert_eq!(
            check_fix(&fix(&[("port", "{= portt =}")]), &[], &generator_of).unwrap_err(),
            "fix use-port: `with` uses `portt`, which is not a slot of `files` or one of dir, stem, file, finding"
        );
    }
}
