//! spellbook's settings: the `[spellbook]` table of `.sorcerer/config.toml`. An unknown key, or an
//! `exclude` entry that is not a glob, is an error naming the key.

use std::collections::BTreeMap;

use globset::Glob;
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

/// The product spellbook's records and state are kept under.
pub const PRODUCT: &str = "spellbook";

/// What is said when spellbook is turned off in the project.
pub const SPELLBOOK_IS_OFF: &str = "spellbook is off in this project ([spellbook] enabled = false)";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SpellbookConfig {
    /// Whether spellbook runs in this project: on by default.
    pub enabled: bool,
    /// Glob patterns of files no rule looks at, such as `gen/**`. Generation ignores them: a
    /// generator writes only what a person asked for by name.
    pub exclude: Vec<String>,
    /// Whether the editor's server refreshes stale regions when a file is saved: on by default.
    /// The team's setting, not the editor's, since it decides whether its lists are kept by hand.
    pub sync_on_save: bool,
    /// `[spellbook.rules]`: how strict the project is with each rule, by its id, over the rule's
    /// own `severity`. Example: `commands-use-ports = "warn"`, or `"off"` to not run it.
    pub rules: BTreeMap<String, RuleSetting>,
}

/// What the project says of one rule in `[spellbook.rules]`: a severity, or `off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSetting {
    Hint,
    Warn,
    Error,
    Off,
}

impl Default for SpellbookConfig {
    fn default() -> SpellbookConfig {
        SpellbookConfig {
            enabled: true,
            exclude: Vec::new(),
            sync_on_save: true,
            rules: BTreeMap::new(),
        }
    }
}

/// The project's spellbook settings: the `[spellbook]` table, with the defaults for keys it
/// leaves out. Every `exclude` pattern must be a glob; otherwise the error names the pattern and
/// the config file.
///
/// Example: `exclude = ["["]` is refused with `[spellbook] exclude: `[` is not a glob (…)`.
pub fn spellbook_config(project: &Project) -> Result<SpellbookConfig> {
    let config: SpellbookConfig = project.config.table(PRODUCT)?;
    match config.exclude_problem() {
        None => Ok(config),
        Some(why) => Err(Error::Config {
            file: project.root.join(".sorcerer/config.toml"),
            why: format!("[spellbook] {why}"),
        }),
    }
}

impl SpellbookConfig {
    fn exclude_problem(&self) -> Option<String> {
        self.exclude.iter().find_map(|pattern| {
            Glob::new(pattern)
                .err()
                .map(|e| format!("exclude: `{pattern}` is not a glob ({e})"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(toml: Option<&str>) -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        // spellbook:allow domain-does-no-io — a test sets up its own files
        std::fs::create_dir_all(dir.path().join(".sorcerer")).unwrap();
        if let Some(text) = toml {
            // spellbook:allow domain-does-no-io — a test sets up its own files
            std::fs::write(dir.path().join(".sorcerer/config.toml"), text).unwrap();
        }
        let project = Project::open(dir.path()).unwrap();
        (dir, project)
    }

    #[test]
    fn defaults_with_no_table() {
        let (_dir, project) = project(None);
        let config = spellbook_config(&project).unwrap();
        assert!(config.enabled);
        assert!(config.exclude.is_empty());
    }

    #[test]
    fn an_unknown_key_is_refused_by_name() {
        let (_dir, project) = project(Some("[spellbook]\nexclud = []\n"));
        let error = spellbook_config(&project).unwrap_err().to_string();
        assert!(error.contains("exclud"), "{error}");
    }

    #[test]
    fn rule_settings_are_read_and_a_wrong_word_is_refused() {
        let (_dir, read) = project(Some("[spellbook.rules]\na = \"warn\"\nb = \"off\"\n"));
        let config = spellbook_config(&read).unwrap();
        assert_eq!(config.rules["a"], RuleSetting::Warn);
        assert_eq!(config.rules["b"], RuleSetting::Off);
        let (_dir, loud) = project(Some("[spellbook.rules]\na = \"loud\"\n"));
        let error = spellbook_config(&loud).unwrap_err().to_string();
        assert!(error.contains("loud") && error.contains("off"), "{error}");
    }

    #[test]
    fn a_bad_exclude_glob_is_named() {
        let (_dir, project) = project(Some("[spellbook]\nexclude = [\"[\"]\n"));
        let error = spellbook_config(&project).unwrap_err().to_string();
        assert!(
            error.contains("exclude") && error.contains("config.toml"),
            "{error}"
        );
    }
}
