//! The project's rules, read from `.sorcerer/spellbook/rules.toml`.

use sorcerer_core::domain::project::Project;

use crate::adapters::generator_dir::generator_named;
use crate::domain::fix::{FixGenerator, check_fix};
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

/// The project's rules, in file order; none when the file is not there. Each rule's fixes are
/// checked against the project's generators.
///
/// Example: a project with no `rules.toml` has no rules; one with a rule that names both
/// `match` and `query` gets "rule x: write `match` or `query`, not both", and one whose fix names
/// a generator the project lacks gets "rule x: fix g: the project has no generator `g`".
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
    let rules = parse_rules(&text).map_err(|why| RulesProblem {
        why: why.strip_prefix("rules.toml: ").unwrap_or(&why).to_string(),
    })?;
    check_fixes(project, &rules)?;
    Ok(rules)
}

/// Every rule's fixes name a generator of the project and only names it can fill.
fn check_fixes(project: &Project, rules: &[Rule]) -> Result<(), RulesProblem> {
    let generator_of = |id: &str| match generator_named(project, id) {
        None => FixGenerator::Absent,
        Some(Err(_)) => FixGenerator::Broken,
        Some(Ok(generator)) => {
            FixGenerator::Takes(generator.inputs.into_iter().map(|(name, _)| name).collect())
        }
    };
    for rule in rules {
        for fix in &rule.fixes {
            check_fix(fix, rule.files.slots(), &generator_of).map_err(|why| RulesProblem {
                why: format!("rule {}: {why}", rule.id),
            })?;
        }
    }
    Ok(())
}
