//! Which file is the project's: the editor's path made relative to the project root.

use std::path::{Path, PathBuf};

use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;

/// The path relative to the project's root, written with `/`, when the file is the project's
/// (the nearest project root above it is this project's).
///
/// The editor's path may differ from the canonical one (a symlinked directory, `/var` and
/// `/private/var`), so the comparison is made on the canonical path; a file not on disk yet is
/// resolved through its folder.
pub(super) fn project_relative(project: &Project, path: &Path) -> Option<PathBuf> {
    let canonical = canonical(path).ok().or_else(|| {
        let folder = canonical(path.parent()?).ok()?;
        Some(folder.join(path.file_name()?))
    })?;
    let directory = canonical.parent()?;
    if Project::root_above(directory)? != project.root {
        return None;
    }
    Some(PathBuf::from(forward_slashes(
        canonical.strip_prefix(&project.root).ok()?,
    )))
}
