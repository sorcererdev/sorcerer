//! The questions a product asks about a codebase, behind one trait so that
//! a service (a graph of the codebase served over an API) can answer them
//! later. Every path is relative to the project root.

use std::path::{Path, PathBuf};

use crate::Result;
use crate::domain::facts::Facts;
use crate::domain::unit::Unit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepHit {
    pub path: PathBuf,
    /// 1-based.
    pub line: u32,
    /// The line without its line ending.
    pub text: String,
}

pub trait CodebaseView: Send + Sync {
    /// The file's text now. Not UTF-8 is [`Error::NotText`].
    fn text(&self, path: &Path) -> Result<String>;
    /// Whether PATH is in the working tree now (text or not).
    fn exists(&self, path: &Path) -> bool;
    /// The file's text at REV; None when it did not exist there.
    fn text_at(&self, path: &Path, rev: &str) -> Result<Option<String>>;
    /// The file's units now; empty when no language covers it.
    fn units(&self, path: &Path) -> Result<Vec<Unit>>;
    /// The file's units at REV; empty when it did not exist there or no
    /// language covers it.
    fn units_at(&self, path: &Path, rev: &str) -> Result<Vec<Unit>>;
    fn facts(&self, path: &Path, unit: &Unit) -> Result<Facts>;
    /// Lines matching PATTERN (a regex), in files matching GLOB when given.
    fn grep(&self, pattern: &str, glob: Option<&str>) -> Result<Vec<GrepHit>>;
    /// Files matching PATTERN, sorted.
    fn glob(&self, pattern: &str) -> Result<Vec<PathBuf>>;
    /// Files that differ from REV in the working tree, plus untracked files
    /// git does not ignore, sorted. A rename is both paths: the old one
    /// deleted, the new one added.
    fn changed_since(&self, rev: &str) -> Result<Vec<PathBuf>>;
    /// The file's change against REV in the working tree, as `git diff` prints
    /// it; empty when there is none (also for an untracked file).
    fn diff(&self, path: &Path, rev: &str) -> Result<String>;
    /// The commit id where HEAD and REV last agree. An unknown REV is
    /// [`Error::Git`] naming it.
    fn merge_base(&self, rev: &str) -> Result<String>;
}
