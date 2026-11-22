//! The editor's view of the project: the open documents' text, unsaved edits included, laid
//! over the files on disk.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
/// The view is a picture of one moment: a file not open is read from the disk once, and the
/// view answers with that text (or that it was not there) for as long as it lives. So a change
/// is made against the text the writer was shown, and a file the disk changed meanwhile is seen
/// to have moved on.
///
/// What a file says now (`text`, `exists`, `units`, `facts`) follows the editor. Its history and
/// the searches (`text_at`, `units_at`, `grep`, `glob`, `changed_since`, `diff`, `merge_base`)
/// follow the disk and git, which know nothing of unsaved text: the writer sees the person's
/// unsaved work through the file it is asked in, its test file and the files it imports.
///
/// Example: with a file open and a function added to it but not saved, `units` of the file lists
/// the new function, while `diff` of the file against `HEAD` is still empty.
pub struct EditorView {
    files: FilesView,
    /// The open documents' text, by path relative to the root written with `/`.
    open: BTreeMap<PathBuf, String>,
    /// The files not open, as the disk had them when first read.
    disk: Mutex<HashMap<PathBuf, OnDisk>>,
}

/// A file not open, as first read from the disk.
#[derive(Debug, Clone)]
enum OnDisk {
    Absent,
    Text(String),
    /// There, but not text (a binary file, a folder): reading it again gives the error.
    NotText,
}

impl EditorView {
    /// A view of the files with the editor's open documents (paths relative to the root) over
    /// them.
    pub fn new(files: FilesView, open: BTreeMap<PathBuf, String>) -> EditorView {
        let open = open
            .into_iter()
            .map(|(path, text)| (PathBuf::from(forward_slashes(&path)), text))
            .collect();
        EditorView {
            files,
            open,
            disk: Mutex::new(HashMap::new()),
        }
    }

    /// The editor's text of a file, when the editor has it open.
    fn open_text(&self, path: &Path) -> Option<&String> {
        self.open.get(&PathBuf::from(forward_slashes(path)))
    }

    /// A file not open, as the disk had it when the view first read it.
    fn on_disk(&self, path: &Path) -> OnDisk {
        let key = PathBuf::from(forward_slashes(path));
        let mut disk = self
            .disk
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        disk.entry(key)
            .or_insert_with(|| match self.files.text(path) {
                Ok(text) => OnDisk::Text(text),
                Err(_) if !self.files.exists(path) => OnDisk::Absent,
                Err(_) => OnDisk::NotText,
            })
            .clone()
    }

    /// A file's text as the view has it: the editor's, else the disk's as first read.
    fn text_now(&self, path: &Path) -> Result<String> {
        if let Some(text) = self.open_text(path) {
            return Ok(text.clone());
        }
        match self.on_disk(path) {
            OnDisk::Text(text) => Ok(text),
            OnDisk::Absent | OnDisk::NotText => self.files.text(path),
        }
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
        self.text_now(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.open_text(path).is_some() || !matches!(self.on_disk(path), OnDisk::Absent)
    }

    fn text_at(&self, path: &Path, rev: &str) -> Result<Option<String>> {
        self.files.text_at(path, rev)
    }

    fn units(&self, path: &Path) -> Result<Vec<Unit>> {
        match self.language_of(path)? {
            Some(language) => units_of(&language, &self.text_now(path)?),
            None => Ok(Vec::new()),
        }
    }

    fn units_at(&self, path: &Path, rev: &str) -> Result<Vec<Unit>> {
        self.files.units_at(path, rev)
    }

    fn facts(&self, path: &Path, unit: &Unit) -> Result<Facts> {
        match self.language_of(path)? {
            Some(language) => facts_of(&language, &self.text_now(path)?, unit),
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
