//! Related files: the files a file imports, found from the language's `imports.scm` query (it
//! captures `@import.path`, nothing else) and its `[imports]` table. Both are data; no language
//! is named here.

use std::path::{Path, PathBuf};

use sorcerer_core::domain::language::Loaded;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Error, Result};
use tree_sitter::{QueryCursor, StreamingIterator};

use crate::domain::language_tables::{ImportsTable, imports_table};
use crate::domain::templates::fill;

/// At most 8 related files go into a situation.
pub const RELATED_FILES: usize = 8;
/// Each related file is cut to its first 120 lines.
pub const RELATED_LINES: usize = 120;

/// The project files `file` imports, with the first 120 lines of each, at most 8, in the order
/// the imports are written. A language with no `imports.scm` has none.
///
/// Example: `from bistro.booking.models.booking import Booking` in a Python `src/` layout
/// relates to `src/bistro/booking/models/booking.py`; `import os` relates to nothing, since
/// no template finds a file for it.
pub fn related_files(
    view: &dyn CodebaseView,
    lang: &Loaded,
    file: &Path,
    text: &str,
) -> Result<Vec<(PathBuf, String)>> {
    let table = imports_table(&lang.language)?;
    let mut related: Vec<PathBuf> = Vec::new();
    for import in imported_paths(lang, text)? {
        let Some(found) = project_file_for(view, &table, &import, &lang.language.unit_separator)
        else {
            continue;
        };
        if found != file && !related.contains(&found) {
            related.push(found);
        }
        if related.len() == RELATED_FILES {
            break;
        }
    }
    related
        .into_iter()
        .map(|path| Ok((first_lines(&view.text(&path)?), path)))
        .map(|read: Result<(String, PathBuf)>| read.map(|(text, path)| (path, text)))
        .collect()
}

/// What the file's `imports.scm` captured as `@import.path`, in file order.
fn imported_paths(lang: &Loaded, text: &str) -> Result<Vec<String>> {
    let Some((_, query)) = lang.queries.iter().find(|(name, _)| name == "imports.scm") else {
        return Ok(Vec::new());
    };
    let Some(tree) = lang.parse(text)? else {
        return Ok(Vec::new());
    };
    let Some(capture) = query.capture_index_for_name("import.path") else {
        if query.capture_names().is_empty() {
            return Ok(Vec::new());
        }
        return Err(Error::Query {
            file: "imports.scm".into(),
            why: "captures no @import.path".into(),
        });
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), text.as_bytes());
    let mut found: Vec<(usize, String)> = Vec::new();
    while let Some(m) = matches.next() {
        for c in m.captures.iter().filter(|c| c.index == capture) {
            if let Ok(words) = c.node.utf8_text(text.as_bytes()) {
                found.push((c.node.start_byte(), words.to_string()));
            }
        }
    }
    found.sort();
    Ok(found.into_iter().map(|(_, words)| words).collect())
}

/// The first project file one import names: its words joined by `/`, tried through each
/// template, then once more without its last word (which may be a name inside a module, such
/// as `Slot` in `crate::models::slot::Slot`).
fn project_file_for(
    view: &dyn CodebaseView,
    table: &ImportsTable,
    import: &str,
    separator: &str,
) -> Option<PathBuf> {
    let mut words: Vec<&str> = import
        .split(separator)
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .collect();
    while words
        .first()
        .is_some_and(|w| table.drop_first.iter().any(|d| d == w))
    {
        words.remove(0);
    }
    let shorter = words.len().saturating_sub(1);
    [words.len(), shorter]
        .into_iter()
        .filter(|n| *n > 0)
        .find_map(|n| first_template_that_exists(view, table, &words[..n].join("/")))
}

fn first_template_that_exists(
    view: &dyn CodebaseView,
    table: &ImportsTable,
    path: &str,
) -> Option<PathBuf> {
    table
        .resolve
        .iter()
        .filter_map(|template| fill(template, &[("path", path)]).ok())
        .map(PathBuf::from)
        .find(|candidate| view.exists(candidate))
}

fn first_lines(text: &str) -> String {
    text.split_inclusive('\n')
        .take(RELATED_LINES)
        .collect::<String>()
}
