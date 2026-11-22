//! conjure's two tables in a language file, read strictly from what core keeps for products:
//!
//! ```toml
//! [test]
//! tests_for = "{= dir =}/{= stem =}_test.ext"   # where a file's tests live
//!
//! [imports]
//! resolve = ["{= path =}.ext", "{= path =}/__init__.ext"]   # where an import's file may be
//! drop_first = ["crate"]                                  # leading words that are no folder
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use sorcerer_core::domain::language::Language;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::{Error, Result};

use crate::domain::templates::fill;

/// `[test]`: where a source file's tests live.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TestTable {
    /// A template over `{= dir =}` (the file's folder) and `{= stem =}` (its name without the
    /// extension). None: no test file is known for this language.
    pub tests_for: Option<String>,
}

/// `[imports]`: how an imported name becomes a project path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ImportsTable {
    /// Templates over `{= path =}` (the import's words joined by `/`), tried in order.
    pub resolve: Vec<String>,
    /// Leading words of an import that name no folder, such as `crate`.
    pub drop_first: Vec<String>,
}

/// The language's `[test]` table; an unknown key is an error naming the language and the table.
pub fn test_table(lang: &Language) -> Result<TestTable> {
    table_of(lang, "test")
}

/// The language's `[imports]` table; an unknown key is an error naming the language and table.
pub fn imports_table(lang: &Language) -> Result<ImportsTable> {
    table_of(lang, "imports")
}

fn table_of<T: DeserializeOwned + Default>(lang: &Language, name: &str) -> Result<T> {
    match lang.rest.get(name) {
        None => Ok(T::default()),
        Some(value) => value
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| Error::Language {
                name: lang.name.clone(),
                why: format!("[{name}]: {}", e.message()),
            }),
    }
}

/// Where the tests of a file would be, from the language's `[test] tests_for`; `None` when the
/// language names no place.
///
/// Example: `src/a/b.ext` with `"{= dir =}/{= stem =}_test.ext"` is `src/a/b_test.ext`.
pub fn tests_path(lang: &Language, file: &Path) -> Result<Option<PathBuf>> {
    let Some(template) = test_table(lang)?.tests_for else {
        return Ok(None);
    };
    let dir = file.parent().map(forward_slashes).unwrap_or_default();
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let filled =
        fill(&template, &[("dir", &dir), ("stem", &stem)]).map_err(|why| Error::Language {
            name: lang.name.clone(),
            why: format!("[test] tests_for: {why}"),
        })?;
    Ok(Some(PathBuf::from(filled.trim_start_matches('/'))))
}
