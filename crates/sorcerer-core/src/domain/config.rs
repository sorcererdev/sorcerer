//! The project's settings file, `.sorcerer/config.toml`, read once.
//!
//! Core reads three tables itself, `[model]`, `[decide]` and `[languages]`, and refuses a key
//! it does not know in them. Every other table belongs to a product (`[scry]`, `[conjure]`, …)
//! and is kept as it is until that product reads it with [`Config::table`], just as strictly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::{Error, Result};

/// Every top-level table some part of sorcerer reads. Any other table name is probably a typo
/// (`[scyr]`): sorcerer warns about it but still runs.
pub const KNOWN_TABLES: &[&str] = &[
    "model",
    "decide",
    "languages",
    "scry",
    "conjure",
    "spellbook",
    "plans",
];

/// What the settings file says. A project with no file gets the defaults.
#[derive(Debug, Clone)]
pub struct Config {
    pub model: ModelConfig,
    pub decide: DecideConfig,
    /// The languages named in `[languages] names`, to load beside the ones sorcerer ships.
    pub languages: Vec<String>,
    /// The products' tables, each read later by its product.
    pub tables: toml::Table,
    /// Warnings to show the user, such as an unknown table name.
    pub warnings: Vec<String>,
    file: PathBuf,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            model: ModelConfig::default(),
            decide: DecideConfig::default(),
            languages: Vec::new(),
            tables: toml::Table::new(),
            warnings: Vec::new(),
            file: PathBuf::from(".sorcerer/config.toml"),
        }
    }
}

/// `[model]`: which model provider to use, the paths no model may read
/// (`not_evidence`), and one `[model.providers.<name>]` table per configured provider.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ModelConfig {
    pub provider: Option<String>,
    pub not_evidence: Vec<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
}

/// One `[model.providers.<name>]` table: how to reach a provider and what its models cost.
/// `kind` says which adapter talks to it, and defaults to the table's name.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProviderConfig {
    pub kind: Option<String>,
    pub url: Option<String>,
    pub api_key_env: Option<String>,
    pub runner: Option<PathBuf>,
    pub aliases: BTreeMap<String, String>,
    /// Dollars per million tokens, `[input, output]`, by alias or model id.
    pub prices: BTreeMap<String, [f64; 2]>,
}

/// `[decide]`: the model that makes the small yes-or-no calls (Jev), and the environment
/// variable holding its key.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DecideConfig {
    pub model: Option<String>,
    pub key_env: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct LanguagesTable {
    names: Vec<String>,
}

impl Config {
    /// Reads the settings from the file's text. `file` is the file's path, used only in errors
    /// and warnings.
    ///
    /// Example: `[model]\nprovider = "claude"\n[scyr]\n` sets the provider and warns
    /// "unknown table [scyr] in .sorcerer/config.toml"; `[model]\nprovder = "claude"\n` is an
    /// error naming `provder`.
    pub fn parse(text: &str, file: &Path) -> Result<Config> {
        let config_error = |why: String| Error::Config {
            file: file.to_path_buf(),
            why,
        };
        let mut tables: toml::Table =
            toml::from_str(text).map_err(|e| config_error(e.message().to_string()))?;
        let model = take_core_table::<ModelConfig>(&mut tables, "model").map_err(config_error)?;
        let decide =
            take_core_table::<DecideConfig>(&mut tables, "decide").map_err(config_error)?;
        let languages =
            take_core_table::<LanguagesTable>(&mut tables, "languages").map_err(config_error)?;
        let warnings = unknown_table_warnings(&tables, file);
        Ok(Config {
            model,
            decide,
            languages: languages.names,
            tables,
            warnings,
            file: file.to_path_buf(),
        })
    }

    /// Reads one product's table into its settings struct, refusing unknown keys. The struct
    /// carries `#[serde(deny_unknown_fields)]`; a project without the table gets the struct's
    /// defaults.
    ///
    /// Example: `config.table::<ScryConfig>("scry")` on `[scry]\nkut = 0.6\n` fails with an
    /// unknown-key error naming `kut` in `[scry]`.
    pub fn table<T: DeserializeOwned + Default>(&self, name: &str) -> Result<T> {
        let Some(value) = self.tables.get(name) else {
            return Ok(T::default());
        };
        value
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| self.product_table_error(name, e.message()))
    }

    /// An unknown key gets its own error, so the message can name it; anything else is a
    /// general config error.
    fn product_table_error(&self, table: &str, message: &str) -> Error {
        match unknown_field(message) {
            Some(key) => Error::UnknownKey {
                table: table.to_string(),
                key,
            },
            None => Error::Config {
                file: self.file.clone(),
                why: format!("[{table}]: {message}"),
            },
        }
    }
}

/// Removes one of core's own tables from the file's tables and reads it. An absent table gives
/// the defaults; the error text names the table.
fn take_core_table<T: DeserializeOwned + Default>(
    tables: &mut toml::Table,
    name: &str,
) -> std::result::Result<T, String> {
    match tables.remove(name) {
        None => Ok(T::default()),
        Some(value) => value
            .try_into()
            .map_err(|e: toml::de::Error| format!("[{name}]: {}", e.message())),
    }
}

/// One warning per table that no part of sorcerer reads.
fn unknown_table_warnings(tables: &toml::Table, file: &Path) -> Vec<String> {
    tables
        .keys()
        .filter(|name| !KNOWN_TABLES.contains(&name.as_str()))
        .map(|name| format!("unknown table [{name}] in {}", file.display()))
        .collect()
}

/// The key named in serde's message "unknown field `kut`, expected …": `kut`.
fn unknown_field(message: &str) -> Option<String> {
    let rest = message.strip_prefix("unknown field `")?;
    Some(rest[..rest.find('`')?].to_string())
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use std::path::Path;

    #[test]
    fn empty_text_is_the_default() {
        let c = Config::parse("", Path::new("c.toml")).unwrap();
        assert!(c.model.provider.is_none() && c.languages.is_empty() && c.warnings.is_empty());
    }

    #[test]
    fn an_unknown_top_level_table_warns_and_does_not_fail() {
        let c = Config::parse("[scyr]\ncut = 0.7\n", Path::new("c.toml")).unwrap();
        assert_eq!(
            c.warnings,
            vec!["unknown table [scyr] in c.toml".to_string()]
        );
    }

    #[test]
    fn an_unknown_key_in_a_core_table_fails() {
        let e = Config::parse("[model]\nprovder = \"claude\"\n", Path::new("c.toml")).unwrap_err();
        assert!(e.to_string().contains("provder"), "{e}");
    }

    #[test]
    fn a_product_reads_its_table_strictly() {
        #[derive(serde::Deserialize, Default, Debug)]
        #[serde(deny_unknown_fields)]
        struct S {
            cut: Option<f64>,
        }
        let c = Config::parse("[scry]\ncut = 0.6\n", Path::new("c.toml")).unwrap();
        assert_eq!(c.table::<S>("scry").unwrap().cut, Some(0.6));
        let c = Config::parse("[scry]\nkut = 0.6\n", Path::new("c.toml")).unwrap();
        let e = c.table::<S>("scry").unwrap_err();
        assert!(matches!(e, Error::UnknownKey { .. } | Error::Config { .. }));
        assert!(e.to_string().contains("kut"), "{e}");
        assert!(Config::default().table::<S>("scry").unwrap().cut.is_none());
    }

    #[test]
    fn languages_is_a_names_list() {
        let c = Config::parse("[languages]\nnames = [\"go\"]\n", Path::new("c.toml")).unwrap();
        assert_eq!(c.languages, vec!["go"]);
    }
}
