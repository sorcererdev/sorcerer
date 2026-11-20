//! Fences: the paths a researcher may never read or send to a model.
//! Secrets (`.env*` anywhere), sorcerer's own state (`.sorcerer/`, which
//! holds other findings and the ledger), git's internals, anything outside
//! the root (a symlink out, `..`), and the user's `[model] not_evidence`
//! globs. A path is resolved (symlinks, `..`) before it is matched, and
//! matching ignores case. When a path cannot be resolved for certain, it is
//! fenced.

use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::{Error, Result};

/// Paths fenced in every project: secret files (`.env`, `.env.local`, … anywhere), sorcerer's
/// own state, and git's internals.
pub const ALWAYS: &[&str] = &["**/.env*", ".sorcerer/**", ".git/**"];

/// The paths of one project that no model may read or be sent.
#[derive(Debug, Clone)]
pub struct Fences {
    pub(crate) root: PathBuf,
    globs: GlobSet,
    patterns: Vec<String>,
}

impl Fences {
    /// The fences of the project at `root` (a path with its symlinks already resolved): the
    /// [`ALWAYS`] patterns plus the user's `[model] not_evidence` entries from
    /// `.sorcerer/config.toml`.
    ///
    /// An entry with no glob characters names a file or a directory and fences everything under
    /// it too, so `docs/plans` fences `docs/plans/today.md`. Matching ignores case. An entry that
    /// is not a valid glob is a config error.
    pub fn new(root: PathBuf, not_evidence: &[String]) -> Result<Fences> {
        let patterns: Vec<String> = ALWAYS
            .iter()
            .map(|pattern| pattern.to_string())
            .chain(not_evidence.iter().flat_map(|entry| directory_style(entry)))
            .collect();
        let globs = case_insensitive_globs(&patterns)?;
        Ok(Fences {
            root,
            globs,
            patterns,
        })
    }

    /// Whether a path, relative to the root and already resolved, may be read and sent.
    /// The empty path (the root itself) never may. [`Fences::allows`] takes any path and
    /// resolves it first.
    pub fn allows_relative(&self, rel: &Path) -> bool {
        !rel.as_os_str().is_empty() && !self.globs.is_match(rel)
    }

    /// The fence patterns as text, to show a model what it may not read and to turn into
    /// `claude`'s deny rules.
    pub fn patterns(&self) -> Vec<String> {
        self.patterns.clone()
    }

    /// The project root the fences are relative to.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// The patterns for one `not_evidence` entry. A glob stays as it is; a plain path (`docs/plans`,
/// `notes/`) becomes two patterns, the path itself and everything under it:
/// `["docs/plans", "docs/plans/**"]`.
fn directory_style(entry: &str) -> Vec<String> {
    if entry.contains(['*', '?', '[', '{']) {
        return vec![entry.to_string()];
    }
    let bare = entry.trim_end_matches('/');
    vec![bare.to_string(), format!("{bare}/**")]
}

/// One glob set matching any of the patterns, ignoring case; `*` also crosses `/`.
fn case_insensitive_globs(patterns: &[String]) -> Result<GlobSet> {
    let globs = patterns
        .iter()
        .try_fold(GlobSetBuilder::new(), |mut set, pattern| {
            set.add(case_insensitive_glob(pattern)?);
            Ok::<_, Error>(set)
        })?;
    globs.build().map_err(|e| config_error(e.to_string()))
}

fn case_insensitive_glob(pattern: &str) -> Result<globset::Glob> {
    GlobBuilder::new(pattern)
        .case_insensitive(true)
        .literal_separator(false)
        .build()
        .map_err(|e| config_error(format!("[model] not_evidence: {e}")))
}

fn config_error(why: String) -> Error {
    Error::Config {
        file: PathBuf::from(".sorcerer/config.toml"),
        why,
    }
}
