//! The files named on the command line, made relative to the project's root.

use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;

/// Each file as the use cases expect it: relative to the project root, written with `/`, with
/// no leading `./`. An absolute path (or one relative to where the command runs) inside the
/// project is made relative; one outside it is refused with a sentence.
///
/// Example: in `/work/bistro`, `./src/a.py` and `/work/bistro/src/a.py` both become `src/a.py`;
/// `/elsewhere/a.py` is refused.
pub(super) fn relative_to_root(
    project: &Project,
    cwd: &Path,
    files: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let root = canonical(&project.root).unwrap_or_else(|_| project.root.clone());
    files
        .iter()
        .map(|file| {
            let joined = without_dots(&cwd.join(file));
            let resolved = canonical(&joined).unwrap_or(joined);
            resolved
                .strip_prefix(&root)
                .map(|inside| PathBuf::from(forward_slashes(inside)))
                .map_err(|_| {
                    anyhow!(
                        "{} is outside the project ({})",
                        file.display(),
                        root.display()
                    )
                })
        })
        .collect()
}

/// The path with `.` removed and each `..` taken against the part before it.
fn without_dots(path: &Path) -> PathBuf {
    let mut kept = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                kept.pop();
            }
            other => kept.push(other),
        }
    }
    kept
}
