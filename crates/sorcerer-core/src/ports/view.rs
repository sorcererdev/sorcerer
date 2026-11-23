//! The questions a product asks about a codebase, behind one trait, so that a service (a graph
//! of the codebase served over an API) could answer them later instead of the disk. Every path
//! is relative to the project root.

use std::path::{Path, PathBuf};

use crate::Result;
use crate::domain::facts::Facts;
use crate::domain::unit::Unit;

/// One line that a search matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepHit {
    pub path: PathBuf,
    /// Counted from 1.
    pub line: u32,
    /// The line without its line ending.
    pub text: String,
}

/// What a product can ask about the project's code.
///
/// `rev` is anything git understands as a commit: `HEAD`, `main`, a commit id.
pub trait CodebaseView: Send + Sync {
    /// The file's text as it is on disk now. A file that is not UTF-8 text is
    /// [`Error::NotText`](crate::Error::NotText).
    fn text(&self, path: &Path) -> Result<String>;
    /// Whether the file is on disk now, text or not.
    fn exists(&self, path: &Path) -> bool;
    /// The file's text at a commit; `None` when the file did not exist there.
    fn text_at(&self, path: &Path, rev: &str) -> Result<Option<String>>;
    /// The file's units as it is now; none when no language covers the file.
    fn units(&self, path: &Path) -> Result<Vec<Unit>>;
    /// The unit holding the line, or when the file does not parse there, the parser's error
    /// region around it (a unit of kind `BrokenRegion`); `None` when neither holds it.
    fn unit_or_broken_region_at(&self, path: &Path, line: u32) -> Result<Option<Unit>>;
    /// The file's units at a commit; none when the file did not exist there or no language
    /// covers it.
    fn units_at(&self, path: &Path, rev: &str) -> Result<Vec<Unit>>;
    /// What the code alone says about one of the file's units (see
    /// [`Facts`](crate::domain::facts::Facts)).
    fn facts(&self, path: &Path, unit: &Unit) -> Result<Facts>;
    /// Every line matching a regular expression, across the project or only in the files a
    /// glob matches. Example: `grep("fn cancel", Some("src/**"))`.
    fn grep(&self, pattern: &str, glob: Option<&str>) -> Result<Vec<GrepHit>>;
    /// The files a glob matches, sorted. Example: `glob("**/*.toml")`.
    fn glob(&self, pattern: &str) -> Result<Vec<PathBuf>>;
    /// The files that differ on disk from a commit, plus new files git does not ignore,
    /// sorted. A renamed file shows up twice: its old path (deleted) and its new one (added).
    fn changed_since(&self, rev: &str) -> Result<Vec<PathBuf>>;
    /// How the file on disk differs from a commit, as `git diff` prints it; empty when it does
    /// not differ, and also for a file git does not track yet.
    fn diff(&self, path: &Path, rev: &str) -> Result<String>;
    /// The last commit that `HEAD` and `rev` share, as a commit id: where a branch split off.
    /// An unknown `rev` is [`Error::Git`](crate::Error::Git) naming it.
    fn merge_base(&self, rev: &str) -> Result<String>;
}
