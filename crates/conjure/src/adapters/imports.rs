//! Related files: the files a file imports, found from the language's `imports.scm` query (it
//! captures `@import.path`, nothing else) and its `[imports]` table, and the files near each
//! import (beside it and using its names; one folder up and declaring it), found by paths and
//! whole words. All of it is data; no language is named here.

use std::path::{Component, Path, PathBuf};

use crate::ports::project_context::RelatedFiles;
use globset::{Glob, GlobSet, GlobSetBuilder};
use sorcerer_core::domain::language::Loaded;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Error, Result};
use tree_sitter::{QueryCursor, StreamingIterator};

use crate::domain::context_choice::{Closeness, mentions_word};
use crate::domain::language_tables::{ImportsTable, imports_table};
use crate::domain::situation::{
    RELATED_FILES, RELATED_FILES_BESIDE, RELATED_FILES_BESIDE_IN_ALL, RelatedFile,
};
use sorcerer_core::domain::templates::fill;

/// The project files `file` imports, at most 8, in the order the imports are written, each
/// followed by the files beside it that use its names (at most 4 for one import, 6 in all);
/// then the file one folder up that declares it; each whole, with how close it is and the
/// names it defines (the step's budget chooses among them later). A language with no
/// `imports.scm` has none.
///
/// A file beside an import is one in the same folder, of the same language, whose text names
/// something the import defines (a whole word: the name of one of its units, up to the unit
/// separator). That is how a port's adapters and its contract test come with the port: the
/// file defining the `Clock` trait brings `clock/in_memory.rs`, which implements `Clock`; a
/// Python `table_repository.py` brings `in_memory_table_repository.py`, which names
/// `TableRepository`. The file one folder up is the first there whose text names the import's
/// folder as a whole word: `ports/mod.rs`, with `pub mod clock;`, for `ports/clock/mod.rs`, so
/// a step that adds a port beside it sees where ports are declared. A file a fence refuses is
/// never read for this, and not named.
///
/// Example: `from bistro.booking.models.booking import Booking` in a Python `src/` layout
/// relates to `src/bistro/booking/models/booking.py` (and any file beside it that names
/// `Booking`); `import os` relates to nothing, since no template finds a file for it.
pub fn related_files(
    view: &dyn CodebaseView,
    lang: &Loaded,
    file: &Path,
    text: &str,
    allowed: &dyn Fn(&Path) -> bool,
) -> Result<RelatedFiles> {
    let (imported, left_out) = imported_project_files(view, lang, file, text, allowed)?;
    let with_nearby = with_nearby_files(view, lang, file, &imported, allowed)?;
    let files = with_nearby
        .into_iter()
        .map(|(path, closeness)| {
            Ok(RelatedFile {
                text: view.text(&path)?,
                names: names_defined_in(view, &path, &lang.language.unit_separator)?,
                closeness,
                path,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(RelatedFiles { files, left_out })
}

/// The project files `file`'s imports name, in file order, at most 8; and those a fence
/// refuses, found before the eighth.
fn imported_project_files(
    view: &dyn CodebaseView,
    lang: &Loaded,
    file: &Path,
    text: &str,
    allowed: &dyn Fn(&Path) -> bool,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let table = imports_table(&lang.language)?;
    let mut related: Vec<PathBuf> = Vec::new();
    let mut left_out: Vec<PathBuf> = Vec::new();
    for import in imported_paths(lang, text)? {
        let Some(found) = project_file_for(view, &table, &import, &lang.language.unit_separator)
        else {
            continue;
        };
        if found == file || related.contains(&found) || left_out.contains(&found) {
            continue;
        }
        if allowed(&found) {
            related.push(found);
        } else {
            left_out.push(found);
        }
        if related.len() == RELATED_FILES {
            break;
        }
    }
    Ok((related, left_out))
}

/// Each imported file followed by the files near it: those beside it that use its names, then
/// the file one folder up that names its folder (the one that declares it: `ports/mod.rs`
/// with `pub mod clock;` for `ports/clock/mod.rs`); none twice and never `file` itself, at
/// most 4 after one import and 6 in all; each with how close it is.
fn with_nearby_files(
    view: &dyn CodebaseView,
    lang: &Loaded,
    file: &Path,
    imported: &[PathBuf],
    allowed: &dyn Fn(&Path) -> bool,
) -> Result<Vec<(PathBuf, Closeness)>> {
    let search = NearbyFiles {
        view,
        of_language: globs_of(&lang.language.files),
        file,
        imported,
        allowed,
    };
    let mut listed: Vec<(PathBuf, Closeness)> = Vec::new();
    let mut nearby_count = 0;
    for import in imported {
        listed.push((import.clone(), Closeness::Imported));
        let room = RELATED_FILES_BESIDE.min(RELATED_FILES_BESIDE_IN_ALL - nearby_count);
        let names = names_defined_in(view, import, &lang.language.unit_separator)?;
        let paths: Vec<PathBuf> = listed.iter().map(|(path, _)| path.clone()).collect();
        let beside = search.beside(import, &names, &paths, room)?;
        let declaring = if beside.len() < room {
            search.declaring(import, &[paths.as_slice(), beside.as_slice()].concat())?
        } else {
            Vec::new()
        };
        nearby_count += beside.len() + declaring.len();
        listed.extend(beside.into_iter().map(|p| (p, Closeness::BesideAnImport)));
        listed.extend(
            declaring
                .into_iter()
                .map(|p| (p, Closeness::DeclaringAnImport)),
        );
    }
    Ok(listed)
}

/// Where files near an import are looked for: the project's files of the import's language,
/// never the file asked in, an import, or a file a fence refuses (which is not read).
struct NearbyFiles<'a> {
    view: &'a dyn CodebaseView,
    of_language: GlobSet,
    file: &'a Path,
    imported: &'a [PathBuf],
    allowed: &'a dyn Fn(&Path) -> bool,
}

impl NearbyFiles<'_> {
    /// The files in the import's folder whose text names one of `names`, at most `room`.
    fn beside(
        &self,
        import: &Path,
        names: &[String],
        listed: &[PathBuf],
        room: usize,
    ) -> Result<Vec<PathBuf>> {
        let Some(folder) = import.parent() else {
            return Ok(Vec::new());
        };
        self.in_folder(folder, listed, room, &|text| {
            names.iter().any(|name| mentions_word(text, name))
        })
    }

    /// The first file in the folder above the import's whose text names the import's folder.
    fn declaring(&self, import: &Path, listed: &[PathBuf]) -> Result<Vec<PathBuf>> {
        let folder = import.parent();
        let (Some(name), Some(above)) = (
            folder
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy()),
            folder.and_then(Path::parent),
        ) else {
            return Ok(Vec::new());
        };
        self.in_folder(above, listed, 1, &|text| mentions_word(text, &name))
    }

    /// The files directly in `folder` that may be shown, are not listed yet and whose text
    /// `wanted` accepts, in path order, at most `room`.
    fn in_folder(
        &self,
        folder: &Path,
        listed: &[PathBuf],
        room: usize,
        wanted: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<PathBuf>> {
        Ok(self
            .view
            .glob(&folder_glob(folder))?
            .into_iter()
            .filter(|path| path.parent() == Some(folder) && self.of_language.is_match(path))
            .filter(|path| path != self.file && !listed.contains(path))
            .filter(|path| !self.imported.contains(path) && (self.allowed)(path))
            .filter(|path| self.view.text(path).is_ok_and(|text| wanted(&text)))
            .take(room)
            .collect())
    }
}

/// The names a file defines: each unit's name up to the unit separator (`Clock` for both
/// `Clock` and `Clock::now`), once each, in file order.
fn names_defined_in(view: &dyn CodebaseView, path: &Path, separator: &str) -> Result<Vec<String>> {
    let mut names: Vec<String> = Vec::new();
    for unit in view.units(path)? {
        let name = unit.name.split(separator).next().unwrap_or_default().trim();
        if !name.is_empty() && !names.iter().any(|seen| seen == name) {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// A glob for the files in a folder (and below: the caller keeps only the folder's own).
fn folder_glob(folder: &Path) -> String {
    match forward_slashes(folder) {
        folder if folder.is_empty() => "*".to_string(),
        folder => format!("{}/*", globset::escape(&folder)),
    }
}

fn globs_of(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
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
        .filter(|candidate| stays_inside(candidate))
        .find(|candidate| view.exists(candidate))
}

/// A template is data a project can write: a candidate that is absolute, or climbs out with
/// `..`, is not a project file and is never looked at.
fn stays_inside(candidate: &Path) -> bool {
    candidate
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}
