//! spellbook's settings: the `[spellbook]` table of `.sorcerer/config.toml`. An unknown key, or an
//! `exclude` entry that is not a glob, is an error naming the key.

use globset::Glob;
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

/// The product spellbook's records and state are kept under.
pub const PRODUCT: &str = "spellbook";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SpellbookConfig {
    /// Whether spellbook runs in this project: on by default.
    pub enabled: bool,
    /// Glob patterns of files no rule looks at, such as `gen/**`. Generation ignores them: a
    /// generator writes only what a person asked for by name.
    pub exclude: Vec<String>,
}

impl Default for SpellbookConfig {
    fn default() -> SpellbookConfig {
        SpellbookConfig {
            enabled: true,
            exclude: Vec::new(),
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
        std::fs::create_dir_all(dir.path().join(".sorcerer")).unwrap();
        if let Some(text) = toml {
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
    fn a_bad_exclude_glob_is_named() {
        let (_dir, project) = project(Some("[spellbook]\nexclude = [\"[\"]\n"));
        let error = spellbook_config(&project).unwrap_err().to_string();
        assert!(error.contains("exclude") && error.contains("config.toml"), "{error}");
    }
}
