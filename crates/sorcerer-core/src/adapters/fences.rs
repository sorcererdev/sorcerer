//! The disk side of the fences: a project's root with its symlinks resolved, and each path
//! resolved (symlinks, `..`) before it is matched. When a path cannot be resolved for certain,
//! it is fenced.

use std::path::{Component, Path, PathBuf};

use crate::domain::fences::Fences;
use crate::domain::project::Project;
use crate::{Error, Result};

impl Fences {
    /// The fences of a project, from its root and its `[model] not_evidence` setting.
    pub fn for_project(p: &Project) -> Result<Fences> {
        let root = p.root.canonicalize().map_err(|e| Error::io(&p.root, e))?;
        Fences::new(root, &p.config.model.not_evidence)
    }

    /// Whether a path, relative to the root or absolute, may be read and sent to a model. A
    /// path that leads out of the project, or cannot be resolved for certain, may not.
    pub fn allows(&self, path: &Path) -> bool {
        match self.relative(path) {
            Some(rel) => self.allows_relative(&rel),
            None => false,
        }
    }

    /// The symlinks in the project (outside `.git/`) that lead out of it or onto a fenced path,
    /// relative to the root. A researcher that cannot check each path itself (`claude -p`)
    /// is told to deny these by name.
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

    /// The path relative to the root once symlinks and `..` are resolved; `None` when it ends
    /// up outside the root or cannot be resolved for certain.
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

/// The path with every symlink and `..` resolved; `None` when that cannot be done for certain.
///
/// A path that does not exist yet (a file about to be written) resolves through its nearest
/// existing ancestor: `src/new/mod.rs` with only `src/` on disk is the resolved `src/` plus
/// `new/mod.rs`. A `..` in the missing part cannot be resolved for certain, nor can a dangling
/// symlink (its target could appear later, anywhere), so both give `None`.
fn resolve(full: &Path) -> Option<PathBuf> {
    if let Ok(resolved) = full.canonicalize() {
        return Some(resolved);
    }
    let (existing, missing) = split_at_nearest_existing(full)?;
    let resolved = existing.canonicalize().ok()?;
    missing
        .into_iter()
        .try_fold(resolved, |mut path, component| match component {
            Component::Normal(name) => {
                path.push(name);
                Some(path)
            }
            Component::CurDir => Some(path),
            _ => None,
        })
}

/// The nearest ancestor of the path that is on disk (a dangling symlink counts), and the parts
/// below it that are not, from the top down.
fn split_at_nearest_existing(path: &Path) -> Option<(&Path, Vec<Component<'_>>)> {
    if path.symlink_metadata().is_ok() {
        return Some((path, Vec::new()));
    }
    let last = path.components().next_back()?;
    let (existing, mut missing) = split_at_nearest_existing(path.parent()?)?;
    missing.push(last);
    Some((existing, missing))
}
