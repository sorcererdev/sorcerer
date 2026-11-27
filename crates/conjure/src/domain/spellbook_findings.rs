//! conjure's reading of spellbook's findings: which of the project's checked rules a change
//! breaks. spellbook prints them (`spellbook check --texts - --json`); products never import
//! each other, so this is conjure's own reader of that output.

use serde::Deserialize;

/// One place a text breaks one of the project's rules, as spellbook prints it.
///
/// Example: `{ rule: "commands-use-ports", file: "src/booking/commands/seat.rs", line: 36,
/// severity: "error", message: "Commands talk to ports", fingerprint: "312ec18" }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RuleFinding {
    pub rule: String,
    /// Relative to the project root, with `/`.
    pub file: String,
    /// From 1.
    pub line: u32,
    /// `error`, `warn` or `hint`; only an error holds a change back.
    pub severity: String,
    pub message: String,
    /// The same for the same code wherever it moves, so an old finding is told from a new one.
    pub fingerprint: String,
}

impl RuleFinding {
    fn is_error(&self) -> bool {
        self.severity == "error"
    }

    fn same_place_in_the_code(&self, other: &RuleFinding) -> bool {
        self.rule == other.rule && self.file == other.file && self.fingerprint == other.fingerprint
    }
}

/// What checking some texts against the project's rules gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesRead {
    Checked(Vec<RuleFinding>),
    /// Nothing could be said, and why: no spellbook installed, no rules in the project, or
    /// spellbook stopped (its sentence).
    NotChecked(String),
}

/// What conjure did about the project's rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RulesOutcome {
    /// The first answer broke no rule (or the rules could not be checked).
    #[default]
    Kept,
    /// The first answer broke these; conjure asked once more, and the second answer broke none.
    AskedAgain(Vec<RuleFinding>),
    /// Both answers broke rules (these are the second's), so nothing lands.
    HeldBack(Vec<RuleFinding>),
}

/// One rule that looks at a file, as `spellbook rules FILE --json` prints it.
///
/// Example: `{ id: "commands-use-ports", severity: "error", message: "Commands talk to ports",
/// why: Some("A command that opens files itself cannot be tested without them") }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RuleNote {
    pub id: String,
    pub severity: String,
    pub message: String,
    #[serde(default)]
    pub why: Option<String>,
}

#[derive(Deserialize)]
struct RulesJson {
    rules: Vec<RuleNote>,
}

/// The rules of `spellbook rules FILE --json`; other fields are ignored.
pub fn rules_of_rules_json(json: &str) -> Result<Vec<RuleNote>, String> {
    serde_json::from_str::<RulesJson>(json)
        .map(|rules| rules.rules)
        .map_err(|e| format!("spellbook's rules did not read: {e}"))
}

/// What the writer reads about the rules of the file it changes, before it writes: each rule
/// with its message and why, and what to do when the code lacks what a rule asks for. Nothing
/// for no rules.
///
/// Example: one rule gives a paragraph that begins `The project's checked rules for this file
/// (a checker runs them on every change; …):` and then ``- commands-use-ports (error): Commands
/// talk to ports``.
pub fn rules_for_the_file_text(rules: &[RuleNote]) -> String {
    if rules.is_empty() {
        return String::new();
    }
    let rows: String = rules
        .iter()
        .map(|rule| {
            let why = rule
                .why
                .as_deref()
                .map(|why| format!(" {}", why.trim()))
                .unwrap_or_default();
            format!("- {} ({}): {}{why}\n", rule.id, rule.severity, rule.message)
        })
        .collect();
    format!(
        "\nThe project's checked rules for this file (a checker runs them on every change, and a change that breaks an error is not used):\n{rows}Keep them. When the code lacks what a rule asks for (a method on an interface, a module, an adapter), add it as part of the step, in the files where it belongs.\n"
    )
}

#[derive(Deserialize)]
struct CheckJson {
    findings: Vec<RuleFinding>,
}

/// The findings of `spellbook check --json`; other fields are ignored.
pub fn findings_of_check_json(json: &str) -> Result<Vec<RuleFinding>, String> {
    serde_json::from_str::<CheckJson>(json)
        .map(|check| check.findings)
        .map_err(|e| format!("spellbook's findings did not read: {e}"))
}

/// The errors in `after` that `before` did not have: a change's new broken rules. Each finding
/// of `before` excuses one finding of `after` at the same code under the same rule, so a change
/// that only moves a line the rule already found breaks nothing new.
///
/// Example: before, one `commands-use-ports` on `fs::write`; after, the same and a second one on
/// `fs::read`: only the second is returned.
pub fn rules_broken(before: &[RuleFinding], after: &[RuleFinding]) -> Vec<RuleFinding> {
    let mut not_yet_excused: Vec<&RuleFinding> = before.iter().collect();
    after
        .iter()
        .filter(|finding| finding.is_error())
        .filter(|finding| {
            match not_yet_excused
                .iter()
                .position(|old| old.same_place_in_the_code(finding))
            {
                Some(at) => {
                    not_yet_excused.remove(at);
                    false
                }
                None => true,
            }
        })
        .cloned()
        .collect()
}

/// Whether any finding is an error, the only kind that holds a change back.
pub fn any_error(findings: &[RuleFinding]) -> bool {
    findings.iter().any(RuleFinding::is_error)
}

/// What the second write reads after its situation: the rules the first answer broke, each with
/// its line of code, and what to do instead. `line_of` gives a file's line as the answer wrote it.
///
/// Example: one break gives a paragraph that begins `Your last answer was not used: a checker
/// runs the project's rules on every change, and it broke these:` and then
/// ``- src/a.rs:36 `std::fs::write(…)` breaks commands-use-ports: Commands talk to ports``.
pub fn broken_rules_text(
    broken: &[RuleFinding],
    line_of: impl Fn(&str, u32) -> Option<String>,
) -> String {
    let rows: String = broken
        .iter()
        .map(|finding| {
            let code = line_of(&finding.file, finding.line)
                .map(|line| format!(" `{}`", line.trim()))
                .unwrap_or_default();
            format!(
                "- {}:{}{code} breaks {}: {}\n",
                finding.file, finding.line, finding.rule, finding.message
            )
        })
        .collect();
    format!(
        "\nYour last answer was not used: a checker runs the project's rules on every change, and it broke these:\n{rows}Take the step again without breaking them. When the code lacks what the rule asks for (a method on an interface, a module, an adapter), add it as part of the step, in the files where it belongs. When the step cannot be taken within the rules, change nothing and say in the goal what is missing.\n"
    )
}

/// The goal of a change held back: the rules its answer broke, each at its place.
///
/// Example: `held back: it breaks commands-use-ports at seat_walk_in.rs:36 (Commands talk to
/// ports), asked twice`.
pub fn held_back_text(broken: &[RuleFinding]) -> String {
    let places: Vec<String> = broken
        .iter()
        .map(|finding| {
            let name = finding.file.rsplit('/').next().unwrap_or(&finding.file);
            format!(
                "{} at {name}:{} ({})",
                finding.rule, finding.line, finding.message
            )
        })
        .collect();
    format!("held back: it breaks {}, asked twice", places.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(rule: &str, line: u32, fingerprint: &str, severity: &str) -> RuleFinding {
        RuleFinding {
            rule: rule.into(),
            file: "src/a/b.x".into(),
            line,
            severity: severity.into(),
            message: "Talk to ports".into(),
            fingerprint: fingerprint.into(),
        }
    }

    #[test]
    fn a_new_error_is_broken_and_an_old_one_moved_is_not() {
        let before = [finding("ports", 4, "f1", "error")];
        let after = [
            finding("ports", 9, "f1", "error"),
            finding("ports", 10, "f2", "error"),
        ];
        assert_eq!(rules_broken(&before, &after), [after[1].clone()]);
    }

    #[test]
    fn the_same_code_twice_where_it_was_once_is_one_new_break() {
        let before = [finding("ports", 4, "f1", "error")];
        let after = [
            finding("ports", 4, "f1", "error"),
            finding("ports", 8, "f1", "error"),
        ];
        assert_eq!(rules_broken(&before, &after).len(), 1);
    }

    #[test]
    fn warnings_and_hints_break_nothing() {
        let after = [finding("a", 1, "f", "warn"), finding("b", 2, "g", "hint")];
        assert_eq!(rules_broken(&[], &after), []);
        assert!(!any_error(&after));
    }

    #[test]
    fn the_check_json_reads_and_ignores_other_fields() {
        let json = r#"{"checked": "2026-11-27T03:28:16Z", "findings": [{"rule": "ports", "file": "src/a/b.x", "line": 36, "severity": "error", "message": "Talk to ports", "fingerprint": "f1", "fixes": []}]}"#;
        assert_eq!(
            findings_of_check_json(json).unwrap(),
            [finding("ports", 36, "f1", "error")]
        );
        assert!(findings_of_check_json("not json").is_err());
    }

    #[test]
    fn the_rules_json_reads_and_the_up_front_text_names_each_rule() {
        let json = r#"{"rules": [{"id": "ports", "severity": "error", "message": "Talk to ports", "why": "Testable."}, {"id": "n", "severity": "warn", "message": "Name it"}]}"#;
        let rules = rules_of_rules_json(json).unwrap();
        let text = rules_for_the_file_text(&rules);
        assert!(
            text.contains("- ports (error): Talk to ports Testable.\n"),
            "{text}"
        );
        assert!(text.contains("- n (warn): Name it\n"), "{text}");
        assert_eq!(rules_for_the_file_text(&[]), "");
        assert!(rules_of_rules_json("nope").is_err());
    }

    #[test]
    fn the_retry_text_names_each_break_with_its_code() {
        let text = broken_rules_text(&[finding("ports", 36, "f1", "error")], |_, line| {
            (line == 36).then(|| "    fs::write(x);".to_string())
        });
        assert!(
            text.contains("- src/a/b.x:36 `fs::write(x);` breaks ports: Talk to ports\n"),
            "{text}"
        );
        assert!(text.contains("change nothing and say in the goal what is missing"));
    }

    #[test]
    fn a_held_back_goal_names_the_file_line_and_message() {
        assert_eq!(
            held_back_text(&[finding("ports", 36, "f1", "error")]),
            "held back: it breaks ports at b.x:36 (Talk to ports), asked twice"
        );
    }
}
