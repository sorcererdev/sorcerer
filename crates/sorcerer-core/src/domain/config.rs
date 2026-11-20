//! `.sorcerer/config.toml`, parsed once. Core owns `[model]`, `[decide]` and
//! `[languages]` and is strict about them; every other table is kept for the
//! product that owns it, which reads it strictly with [`Config::table`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::{Error, Result};

/// Every top-level table the suite knows. Another name is probably a typo:
/// it is warned about, not refused.
pub const KNOWN_TABLES: &[&str] = &[
    "model",
    "decide",
    "languages",
    "scry",
    "conjure",
    "spellbook",
    "plans",
];

#[derive(Debug, Clone)]
pub struct Config {
    pub model: ModelConfig,
    pub decide: DecideConfig,
    pub languages: Vec<String>,
    /// Every other top-level table, for the products.
    pub tables: toml::Table,
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

/// `[model]` and `[model.providers.*]`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ModelConfig {
    pub provider: Option<String>,
    pub not_evidence: Vec<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
}

/// One `[model.providers.<name>]` table. `kind` defaults to the table's name.
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

/// `[decide]`: the decider (Jev).
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
    /// Parses TEXT, the contents of FILE (named in errors and warnings).
    pub fn parse(text: &str, file: &Path) -> Result<Config> {
        let bad = |why: String| Error::Config {
            file: file.to_path_buf(),
            why,
        };
        let mut tables: toml::Table =
            toml::from_str(text).map_err(|e| bad(e.message().to_string()))?;
        let model = core_table::<ModelConfig>(&mut tables, "model").map_err(bad)?;
        let decide = core_table::<DecideConfig>(&mut tables, "decide").map_err(bad)?;
        let languages = core_table::<LanguagesTable>(&mut tables, "languages").map_err(bad)?;
        let warnings = tables
            .keys()
            .filter(|k| !KNOWN_TABLES.contains(&k.as_str()))
            .map(|k| format!("unknown table [{k}] in {}", file.display()))
            .collect();
        Ok(Config {
            model,
            decide,
            languages: languages.names,
            tables,
            warnings,
            file: file.to_path_buf(),
        })
    }

    /// A product's table, read strictly: its struct carries
    /// `#[serde(deny_unknown_fields)]`. An absent table is `T::default()`.
    pub fn table<T: DeserializeOwned + Default>(&self, name: &str) -> Result<T> {
        let Some(value) = self.tables.get(name) else {
            return Ok(T::default());
        };
        value.clone().try_into().map_err(|e: toml::de::Error| {
            let msg = e.message().to_string();
            match unknown_field(&msg) {
                Some(key) => Error::UnknownKey {
                    table: name.to_string(),
                    key,
                },
                None => Error::Config {
                    file: self.file.clone(),
                    why: format!("[{name}]: {msg}"),
                },
            }
        })
    }
}

/// Takes core's table NAME out of TABLES and reads it.
fn core_table<T: DeserializeOwned + Default>(
    tables: &mut toml::Table,
    name: &str,
) -> std::result::Result<T, String> {
    match tables.remove(name) {
        None => Ok(T::default()),
        Some(v) => v
            .try_into()
            .map_err(|e: toml::de::Error| format!("[{name}]: {}", e.message())),
    }
}

/// The key serde names in "unknown field `x`, expected …".
fn unknown_field(msg: &str) -> Option<String> {
    let rest = msg.strip_prefix("unknown field `")?;
    Some(rest[..rest.find('`')?].to_string())
}

#[cfg(test)]
#[allow(clippy::too_many_lines)]
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
