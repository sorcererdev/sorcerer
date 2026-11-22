//! scry's steering file as conjure's rules.

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;

use crate::domain::rules::rules_text;

/// Where scry's steering lives, relative to the project root.
const STEERING_FILE: &str = ".sorcerer/scry.toml";

/// The project's rules for the writer, from `.sorcerer/scry.toml`: its context, the files it
/// includes (one outside the fences is skipped) and its rules. No file, or one that does not
/// parse (scry says so, not conjure), is no rules.
pub fn project_rules(project: &Project, fences: &Fences) -> String {
    let Ok(text) = std::fs::read_to_string(project.root.join(STEERING_FILE)) else {
        return String::new();
    };
    let include = |path: &std::path::Path| {
        if !fences.allows(path) {
            return None;
        }
        std::fs::read_to_string(project.root.join(path)).ok()
    };
    rules_text(&text, include).unwrap_or_default()
}
