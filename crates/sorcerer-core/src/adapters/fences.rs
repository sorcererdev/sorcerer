//! The disk side of the fences: a project's root with its symlinks resolved, and each path
//! resolved (symlinks, `..`) before it is matched. When a path cannot be resolved for certain,
//! it is fenced.

use std::path::{Component, Path, PathBuf};

use crate::adapters::platform::paths::canonical;
use crate::domain::fences::{Fences, NOTES_DIR, SORCERER_DIR_FENCE};
use crate::domain::project::Project;
use crate::{Error, Result};

impl Fences {
    /// The fences of a project, from its root and its `[model] not_evidence` setting.
    pub fn for_project(p: &Project) -> Result<Fences> {
        let root = canonical(&p.root).map_err(|e| Error::io(&p.root, e))?;
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

    /// The symlinks in the project that lead out of it or onto a fenced path, relative to the
    /// root. A researcher that cannot check each path itself (`claude -p`) is told to deny these
    /// by name. The walk is [`link_walk`]'s; a caller that asks at every task start keeps a
    /// [`FencedLinksCache`](crate::adapters::fenced_links_cache::FencedLinksCache) instead.
    pub fn fenced_links(&self) -> Vec<PathBuf> {
        link_walk(&self.root, |_| true)
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

    /// The patterns for a tool that matches paths by glob alone and cannot make an exception
    /// (`claude`'s deny rules): [`Fences::patterns`] with `.sorcerer/**` replaced by each entry
    /// of `.sorcerer/` but the notes, as it stands now, and the entries sorcerer always keeps
    /// there (`state`, `config.toml`, `spellbook`, `scry.toml`, `scry.md`, `languages`), there
    /// or not. Only an entry named exactly `notes` is left out. A directory is
    /// named both as itself and as everything under it.
    ///
    /// Example: with `.sorcerer/` holding `config.toml`, `notes/` and `state/`, the patterns
    /// hold `.sorcerer/config.toml`, `.sorcerer/config.toml/**`, `.sorcerer/state`,
    /// `.sorcerer/state/**`, … and nothing that names `.sorcerer/notes`.
    pub fn deny_patterns(&self) -> Vec<String> {
        self.patterns()
            .into_iter()
            .flat_map(|pattern| match pattern.as_str() {
                SORCERER_DIR_FENCE => self.sorcerer_entries_but_notes(),
                _ => vec![pattern],
            })
            .collect()
    }

    /// Each entry of `.sorcerer/` but the notes (the kept ones always), as `.sorcerer/<name>`
    /// and `.sorcerer/<name>/**`, in name order, each once.
    fn sorcerer_entries_but_notes(&self) -> Vec<String> {
        let notes = NOTES_DIR.trim_start_matches(".sorcerer/");
        let on_disk = std::fs::read_dir(self.root.join(".sorcerer"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string));
        let names: std::collections::BTreeSet<String> = KEPT_IN_SORCERER_DIR
            .iter()
            .map(|name| name.to_string())
            .chain(on_disk)
            .filter(|name| name != notes)
            .collect();
        names
            .into_iter()
            .flat_map(|name| [format!(".sorcerer/{name}"), format!(".sorcerer/{name}/**")])
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

/// The entries sorcerer keeps in `.sorcerer/`, denied by name to a glob-only tool even before
/// they are made.
const KEPT_IN_SORCERER_DIR: [&str; 6] = [
    "state",
    "config.toml",
    "spellbook",
    "scry.toml",
    "scry.md",
    "languages",
];

/// The entries of the walk for fenced links: everything under the root that `keep` keeps (it is
/// given each path relative to the root, and a directory it refuses is not entered), without
/// following links, leaving out every `.git` and the root's `.sorcerer/` but its notes (fenced
/// as a whole, so no link in them needs naming; the notes are walked like any directory).
///
/// No ignore file is read (`.gitignore`, `.ignore`, git's excludes): git checks out an ignored
/// file that was added with `git add -f`, and a link made in an ignored directory is still there
/// for `claude` to read through, so a link is named wherever it is. That also keeps the walk's
/// result a function of the directories' entries alone, which the cache relies on.
///
/// Each item is an entry, or the error of a directory that could not be read
/// (the caller decides what an unread directory means; the cache walks it again next time).
///
/// Example: with `target/` in the root's `.gitignore`, `target/out -> /etc` and `src/out -> /etc`
/// are both walked.
pub(crate) fn link_walk(
    root: &Path,
    keep: impl Fn(&Path) -> bool + Send + Sync + 'static,
) -> impl Iterator<Item = std::result::Result<ignore::DirEntry, ignore::Error>> {
    let root_for_filter = root.to_path_buf();
    ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .follow_links(false)
        .filter_entry(move |e| {
            let Ok(rel) = e.path().strip_prefix(&root_for_filter) else {
                return false;
            };
            let fenced_whole = e.file_name() == ".git" || in_sorcerer_dir_but_not_notes(rel);
            !fenced_whole && keep(rel)
        })
        .build()
}

/// Whether a walked path (relative to the root) is in the root's `.sorcerer/` and not the
/// notes: `.sorcerer/state` is, `.sorcerer` itself and `.sorcerer/notes/a.md` are not.
fn in_sorcerer_dir_but_not_notes(rel: &Path) -> bool {
    let mut names = rel.components();
    names.next() == Some(Component::Normal(".sorcerer".as_ref()))
        && names.next().is_some()
        && !crate::domain::fences::is_in_notes(rel)
}

/// The path with every symlink and `..` resolved; `None` when that cannot be done for certain.
///
/// A path that does not exist yet (a file about to be written) resolves through its nearest
/// existing ancestor: `src/new/mod.rs` with only `src/` on disk is the resolved `src/` plus
/// `new/mod.rs`. A `..` in the missing part cannot be resolved for certain, nor can a dangling
/// symlink (its target could appear later, anywhere), so both give `None`.
fn resolve(full: &Path) -> Option<PathBuf> {
    if let Ok(resolved) = canonical(full) {
        return Some(resolved);
    }
    let (existing, missing) = split_at_nearest_existing(full)?;
    let resolved = canonical(existing).ok()?;
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
