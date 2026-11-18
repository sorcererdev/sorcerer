//! Fences: the paths a researcher may never read or send to a model.
//! Secrets (`.env*` anywhere), sorcerer's own state (`.sorcerer/`, which
//! holds other findings and the ledger), git's internals, anything outside
//! the root (a symlink out, `..`), and the user's `[model] not_evidence`
//! globs. A path is resolved (symlinks, `..`) before it is matched, and
//! matching ignores case. When a path cannot be resolved for certain, it is
//! fenced.

use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::{Error, Project, Result};

/// Fenced in every project.
pub const ALWAYS: &[&str] = &["**/.env*", ".sorcerer/**", ".git/**"];

#[derive(Debug, Clone)]
pub struct Fences {
    root: PathBuf,
    globs: GlobSet,
    patterns: Vec<String>,
}

impl Fences {
    pub fn for_project(p: &Project) -> Result<Fences> {
        let root = p.root.canonicalize().map_err(|e| Error::io(&p.root, e))?;
        let patterns: Vec<String> = ALWAYS
            .iter()
            .map(|s| s.to_string())
            .chain(
                p.config
                    .model
                    .not_evidence
                    .iter()
                    .flat_map(|g| directory_style(g)),
            )
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

    /// Whether PATH (relative to the root, or absolute) may be read and sent.
    pub fn allows(&self, path: &Path) -> bool {
        match self.relative(path) {
            Some(rel) => !rel.as_os_str().is_empty() && !self.globs.is_match(&rel),
            None => false,
        }
    }

    /// The patterns, for prompts and `claude`'s deny rules.
    pub fn patterns(&self) -> Vec<String> {
        self.patterns.clone()
    }

    /// Symlinks in the project (outside `.git/`) that lead out of the root
    /// or onto a fenced path, relative to the root. A researcher that cannot
    /// check each path itself (`claude -p`) denies these by name.
    pub fn fenced_links(&self) -> Vec<PathBuf> {
        ignore::WalkBuilder::new(&self.root)
            .hidden(false)
            .follow_links(false)
            .filter_entry(|e| e.file_name() != ".git")
            .build()
            .flatten()
            .filter(|e| e.path_is_symlink())
            .filter_map(|e| {
                e.path()
                    .strip_prefix(&self.root)
                    .ok()
                    .map(Path::to_path_buf)
            })
            .filter(|rel| !self.allows(rel))
            .collect()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// PATH inside the root after resolving symlinks and `..`; None when it
    /// is outside, or cannot be resolved for certain.
    pub fn relative(&self, path: &Path) -> Option<PathBuf> {
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let resolved = resolve(&full)?;
        resolved
            .strip_prefix(&self.root)
            .ok()
            .map(Path::to_path_buf)
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

/// FULL with every symlink and `..` resolved. A path that does not exist
/// yet resolves through its nearest existing ancestor; a `..` in the part
/// that does not exist cannot be resolved for certain, so it is None, and
/// so is a dangling symlink (its target could appear later, anywhere).
fn resolve(full: &Path) -> Option<PathBuf> {
    if let Ok(c) = full.canonicalize() {
        return Some(c);
    }
    let mut existing = full;
    let mut rest: Vec<Component> = Vec::new();
    loop {
        if existing.symlink_metadata().is_ok() {
            break;
        }
        rest.push(existing.components().next_back()?);
        existing = existing.parent()?;
    }
    let mut out = existing.canonicalize().ok()?;
    for c in rest.into_iter().rev() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}
