//! The project's say over its rules: `[spellbook.rules]` in `.sorcerer/config.toml` gives a rule
//! another severity, or turns it off, without editing the rule.

use std::collections::BTreeMap;

use crate::domain::config::RuleSetting;
use crate::domain::rule::{Rule, Severity};

/// The rules as the project runs them: each with the severity the project gives it, and the ids
/// of the rules it turned off (an allow comment naming one is still a known rule).
#[derive(Debug, Clone, Default)]
pub struct ProjectRules {
    pub run: Vec<Rule>,
    pub off: Vec<String>,
}

/// The rules with the project's settings over them. Every setting must name a rule; otherwise the
/// sentence names it and lists the rules.
///
/// Example: with `commands-use-ports = "warn"`, that rule's findings are warnings; with `"off"`, it
/// is not run and `off` lists it; `comands-use-ports = "warn"` (a typo) is
/// ``[spellbook.rules]: `comands-use-ports` names no rule (the rules are: commands-use-ports)``.
pub fn with_project_settings(
    rules: Vec<Rule>,
    settings: &BTreeMap<String, RuleSetting>,
) -> Result<ProjectRules, String> {
    if let Some(unknown) = settings
        .keys()
        .find(|id| !rules.iter().any(|rule| &rule.id == *id))
    {
        return Err(names_no_rule_text(unknown, &rules));
    }
    let (off, run): (Vec<Rule>, Vec<Rule>) = rules
        .into_iter()
        .partition(|rule| settings.get(&rule.id) == Some(&RuleSetting::Off));
    Ok(ProjectRules {
        run: run
            .into_iter()
            .map(|rule| with_setting(rule, settings))
            .collect(),
        off: off.into_iter().map(|rule| rule.id).collect(),
    })
}

fn with_setting(rule: Rule, settings: &BTreeMap<String, RuleSetting>) -> Rule {
    let severity = match settings.get(&rule.id) {
        Some(RuleSetting::Hint) => Severity::Hint,
        Some(RuleSetting::Warn) => Severity::Warn,
        Some(RuleSetting::Error) => Severity::Error,
        Some(RuleSetting::Off) | None => rule.severity,
    };
    Rule { severity, ..rule }
}

fn names_no_rule_text(unknown: &str, rules: &[Rule]) -> String {
    let ids: Vec<&str> = rules.iter().map(|rule| rule.id.as_str()).collect();
    if ids.is_empty() {
        return format!("[spellbook.rules]: `{unknown}` names no rule (this project has no rules)");
    }
    format!(
        "[spellbook.rules]: `{unknown}` names no rule (the rules are: {})",
        ids.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::rule::parse_rules;

    fn rules() -> Vec<Rule> {
        parse_rules(
            "[rules.a]\nfiles = \"**\"\nmatch = { kind = \"x\" }\nmessage = \"A\"\n\
             [rules.b]\nfiles = \"**\"\nmatch = { kind = \"x\" }\nmessage = \"B\"\nseverity = \"hint\"\n",
        )
        .unwrap()
    }

    fn settings(pairs: &[(&str, RuleSetting)]) -> BTreeMap<String, RuleSetting> {
        pairs.iter().map(|(id, s)| (id.to_string(), *s)).collect()
    }

    #[test]
    fn a_setting_replaces_the_rules_severity() {
        let project = with_project_settings(
            rules(),
            &settings(&[("a", RuleSetting::Warn), ("b", RuleSetting::Error)]),
        )
        .unwrap();
        let severities: Vec<Severity> = project.run.iter().map(|rule| rule.severity).collect();
        assert_eq!(severities, [Severity::Warn, Severity::Error]);
        assert!(project.off.is_empty());
    }

    #[test]
    fn off_leaves_the_rule_out_and_lists_it() {
        let project =
            with_project_settings(rules(), &settings(&[("a", RuleSetting::Off)])).unwrap();
        let run: Vec<&str> = project.run.iter().map(|rule| rule.id.as_str()).collect();
        assert_eq!(run, ["b"]);
        assert_eq!(project.off, ["a"]);
    }

    #[test]
    fn a_setting_for_no_rule_is_named() {
        let why =
            with_project_settings(rules(), &settings(&[("c", RuleSetting::Warn)])).unwrap_err();
        assert_eq!(
            why,
            "[spellbook.rules]: `c` names no rule (the rules are: a, b)"
        );
    }
}
