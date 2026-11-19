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

/// Fenced in every project.
pub const ALWAYS: &[&str] = &["**/.env*", ".sorcerer/**", ".git/**"];

#[derive(Debug, Clone)]
pub struct Fences {
    pub(crate) root: PathBuf,
    globs: GlobSet,
    patterns: Vec<String>,
}

impl Fences {
    /// The fences of a project at ROOT (already resolved) with the user's
    /// `[model] not_evidence` globs.
    pub fn new(root: PathBuf, not_evidence: &[String]) -> Result<Fences> {
        let patterns: Vec<String> = ALWAYS
            .iter()
            .map(|s| s.to_string())
            .chain(not_evidence.iter().flat_map(|g| directory_style(g)))
            .collect();
        let mut set = GlobSetBuilder::new();
        for pat in &patterns {
            let g = GlobBuilder::new(pat)
                .case_insensitive(true)
                .literal_separator(false)
                .build()
                .map_err(|e| Error::Config {
                    file: PathBuf::from(".sorcerer/config.toml"),
                    why: format!("[model] not_evidence: {e}"),
                })?;
            set.add(g);
        }
        let globs = set.build().map_err(|e| Error::Config {
            file: PathBuf::from(".sorcerer/config.toml"),
            why: e.to_string(),
        })?;
        Ok(Fences {
            root,
            globs,
            patterns,
        })
    }

    /// Whether REL, a path already resolved inside the root, may be read
    /// and sent ([`Fences::allows`] resolves one first).
    pub fn allows_relative(&self, rel: &Path) -> bool {
        !rel.as_os_str().is_empty() && !self.globs.is_match(rel)
    }

    /// The patterns, for prompts and `claude`'s deny rules.
    pub fn patterns(&self) -> Vec<String> {
        self.patterns.clone()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// A `not_evidence` entry with no glob characters names a directory or a
/// file (`docs/plans`, `notes/`): it fences the path and everything under it.
fn directory_style(g: &str) -> Vec<String> {
    if g.contains(['*', '?', '[', '{']) {
        return vec![g.to_string()];
    }
    let bare = g.trim_end_matches('/');
    vec![bare.to_string(), format!("{bare}/**")]
}
