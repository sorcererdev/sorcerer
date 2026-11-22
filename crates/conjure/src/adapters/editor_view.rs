//! The editor's view of the project: the open documents' text, unsaved edits included, laid
//! over the files on disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sorcerer_core::Result;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::domain::facts::{Facts, facts_of};
use sorcerer_core::domain::language::Loaded;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::{Unit, units_of};
use sorcerer_core::ports::view::{CodebaseView, GrepHit};

/// The project as the editor shows it now: a file the editor has open is read from the
/// editor's text, saved or not; every other file from the disk.
///
/// What a file says now (`text`, `exists`, `units`, `facts`) follows the editor. Its history and
/// the searches (`text_at`, `units_at`, `grep`, `glob`, `changed_since`, `diff`, `merge_base`)
/// follow the disk and git, which know nothing of unsaved text: the writer sees the person's
/// unsaved work through the file it is asked in, its test file and the files it imports.
///
/// Example: with `src/cart.py` open and a function added but not saved, `units("src/cart.py")`
/// lists the new function, while `diff("src/cart.py", "HEAD")` is still empty.
pub struct EditorView {
    files: FilesView,
    /// The open documents' text, by path relative to the root written with `/`.
    open: BTreeMap<PathBuf, String>,
}

impl EditorView {
    /// A view of the files with the editor's open documents (paths relative to the root) over
    /// them.
    pub fn new(files: FilesView, open: BTreeMap<PathBuf, String>) -> EditorView {
        let open = open
            .into_iter()
            .map(|(path, text)| (PathBuf::from(forward_slashes(&path)), text))
            .collect();
        EditorView { files, open }
    }

    /// The editor's text of a file, when the editor has it open.
    fn open_text(&self, path: &Path) -> Option<&String> {
        self.open.get(&PathBuf::from(forward_slashes(path)))
    }

    /// The file's language, loaded; `None` when no language covers the file.
    fn language_of(&self, path: &Path) -> Result<Option<std::sync::Arc<Loaded>>> {
        let project = self.files.project();
        let Some(language) = project.languages.for_path(path) else {
            return Ok(None);
        };
        Ok(Some(
            project.languages.load(&language.name, self.files.cache())?,
        ))
    }
}

impl CodebaseView for EditorView {
    fn text(&self, path: &Path) -> Result<String> {
        match self.open_text(path) {
            Some(text) => Ok(text.clone()),
            None => self.files.text(path),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.open_text(path).is_some() || self.files.exists(path)
    }

    fn text_at(&self, path: &Path, rev: &str) -> Result<Option<String>> {
        self.files.text_at(path, rev)
    }

    fn units(&self, path: &Path) -> Result<Vec<Unit>> {
        let Some(text) = self.open_text(path) else {
            return self.files.units(path);
        };
        match self.language_of(path)? {
            Some(language) => units_of(&language, text),
            None => Ok(Vec::new()),
        }
    }

    fn units_at(&self, path: &Path, rev: &str) -> Result<Vec<Unit>> {
        self.files.units_at(path, rev)
    }

    fn facts(&self, path: &Path, unit: &Unit) -> Result<Facts> {
        let Some(text) = self.open_text(path) else {
            return self.files.facts(path, unit);
        };
        match self.language_of(path)? {
            Some(language) => facts_of(&language, text, unit),
            None => Ok(Facts::default()),
        }
    }

    fn grep(&self, pattern: &str, glob: Option<&str>) -> Result<Vec<GrepHit>> {
        self.files.grep(pattern, glob)
    }

    fn glob(&self, pattern: &str) -> Result<Vec<PathBuf>> {
        self.files.glob(pattern)
    }

    fn changed_since(&self, rev: &str) -> Result<Vec<PathBuf>> {
        self.files.changed_since(rev)
    }

    fn diff(&self, path: &Path, rev: &str) -> Result<String> {
        self.files.diff(path, rev)
    }

    fn merge_base(&self, rev: &str) -> Result<String> {
        self.files.merge_base(rev)
    }
}
