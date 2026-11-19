//! The fences' disk side: a project's resolved root, and paths resolved
//! (symlinks, `..`) before they are matched. When a path cannot be resolved
//! for certain, it is fenced.

use std::path::{Component, Path, PathBuf};

use crate::domain::fences::Fences;
use crate::domain::project::Project;
use crate::{Error, Result};

impl Fences {
    pub fn for_project(p: &Project) -> Result<Fences> {
        let root = p.root.canonicalize().map_err(|e| Error::io(&p.root, e))?;
        Fences::new(root, &p.config.model.not_evidence)
    }

    /// Whether PATH (relative to the root, or absolute) may be read and sent.
    pub fn allows(&self, path: &Path) -> bool {
        match self.relative(path) {
            Some(rel) => self.allows_relative(&rel),
            None => false,
        }
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
