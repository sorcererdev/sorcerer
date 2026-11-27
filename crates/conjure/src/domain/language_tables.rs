//! conjure's two tables in a language file, read strictly from what core keeps for products:
//!
//! ```toml
//! [test]
//! tests_for = "{= dir =}/{= stem =}_test.ext"   # where a file's tests live
//! dir_replace = ["src/main/", "src/test/"]      # optional: the tests' folder tree beside
//!
//! [imports]
//! resolve = ["{= path =}.ext", "{= path =}/__init__.ext"]   # where an import's file may be
//! drop_first = ["crate"]                                  # leading words that are no folder
//! separator = "/"                                         # optional: between an import's words
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use sorcerer_core::domain::language::Language;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::{Error, Result};

use sorcerer_core::domain::templates::fill;

/// `[test]`: where a source file's tests live.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TestTable {
    /// A template over `{= dir =}` (the file's folder) and `{= stem =}` (its name without the
    /// extension). None: no test file is known for this language.
    pub tests_for: Option<String>,
    /// `[from, to]`: the tests live in a folder tree beside the code's, so `{= dir =}` is the
    /// file's folder with its first `from` replaced by `to`. A file whose folder holds no
    /// `from` has no known test file. None: `{= dir =}` is the file's own folder.
    pub dir_replace: Option<[String; 2]>,
}

/// `[imports]`: how an imported name becomes a project path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ImportsTable {
    /// Templates over `{= path =}` (the import's words joined by `/`) and `{= dir =}` (the
    /// importing file's folder), tried in order. A `.` or `..` in the filled path is folded
    /// away (`booking/./models.ts` is `booking/models.ts`); a path that climbs out of the
    /// project is never looked at.
    pub resolve: Vec<String>,
    /// Leading words of an import that name no folder, such as `crate`.
    pub drop_first: Vec<String>,
    /// What separates an import's words, when it is not the language's unit separator: `/`
    /// for `./models/booking`.
    pub separator: Option<String>,
}

impl ImportsTable {
    /// What separates an import's words: `separator`, else the language's unit separator.
    pub fn word_separator<'a>(&'a self, lang: &'a Language) -> &'a str {
        self.separator.as_deref().unwrap_or(&lang.unit_separator)
    }
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

/// Where the tests of a file would be, from the language's `[test] tests_for` (and
/// `dir_replace`); `None` when the language names no place.
///
/// Example: `src/a/b.ext` with `"{= dir =}/{= stem =}_test.ext"` is `src/a/b_test.ext`; with
/// `dir_replace = ["src/main/", "src/test/"]` and `"{= dir =}/{= stem =}Test.ext"`,
/// `src/main/a/B.ext` is `src/test/a/BTest.ext`.
pub fn tests_path(lang: &Language, file: &Path) -> Result<Option<PathBuf>> {
    let table = test_table(lang)?;
    let Some(template) = table.tests_for else {
        return Ok(None);
    };
    let folder = file.parent().map(forward_slashes).unwrap_or_default();
    let Some(dir) = tests_folder(&folder, table.dir_replace.as_ref()) else {
        return Ok(None);
    };
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

/// The folder a file's tests are in: its own, or with `dir_replace`'s first `from` made `to`
/// (`None` when the folder holds no `from`).
fn tests_folder(folder: &str, dir_replace: Option<&[String; 2]>) -> Option<String> {
    let Some([from, to]) = dir_replace else {
        return Some(folder.to_string());
    };
    let with_slash = format!("{folder}/");
    with_slash
        .contains(from.as_str())
        .then(|| with_slash.replacen(from.as_str(), to, 1))
        .map(|replaced| replaced.trim_end_matches('/').to_string())
}
