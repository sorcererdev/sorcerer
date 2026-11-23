//! The project's rules, read from `.sorcerer/spellbook/rules.toml`.

use sorcerer_core::domain::project::Project;

use crate::domain::rule::{Rule, parse_rules};

/// Where the rules are kept, relative to the project root.
pub const RULES_FILE: &str = ".sorcerer/spellbook/rules.toml";

/// Why the rules could not be loaded: the file and the sentence that says what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesProblem {
    pub why: String,
}

impl std::fmt::Display for RulesProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{RULES_FILE}: {}", self.why)
    }
}

/// The project's rules, in file order; none when the file is not there.
///
/// Example: a project with no `rules.toml` has no rules; one with a rule that names both
/// `match` and `query` gets "rule x: write `match` or `query`, not both".
pub fn load_rules(project: &Project) -> Result<Vec<Rule>, RulesProblem> {
    let path = project.root.join(RULES_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(RulesProblem {
                why: format!("cannot be read ({e})"),
            });
        }
    };
    parse_rules(&text).map_err(|why| RulesProblem {
        why: why.strip_prefix("rules.toml: ").unwrap_or(&why).to_string(),
    })
}
