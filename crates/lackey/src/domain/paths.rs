//! Which paths a task may name: a path the agent wrote, relative to the project root after
//! lexical clean-up (conjure's rule, copied: products never import each other), and the paths
//! no task writes (`library/never-written.toml`).

use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use serde::Deserialize;
use sorcerer_core::domain::fences::is_in_notes;

/// The path the agent named, relative to the project root after lexical clean-up: `.`
/// dropped, `..` taken as far as the root and no further; an absolute path only when it is
/// under `root`. `None` when it is empty, leaves the root or is absolute elsewhere, or holds a
/// drive, a stream or a backslash (no one name on every platform).
///
/// Example: `src/./a.txt` is `src/a.txt`; `a/../../x` and `/etc/passwd` are `None`.
pub fn project_relative(root: &Path, written: &str) -> Option<PathBuf> {
    let path = Path::new(written);
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let mut kept: Vec<&std::ffi::OsStr> = Vec::new();
    for part in relative.components() {
        match part {
            Component::Normal(name) if name.to_string_lossy().contains(['\\', ':']) => {
                return None;
            }
            Component::Normal(name) => kept.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                kept.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!kept.is_empty()).then(|| kept.into_iter().collect())
}

/// The shipped list, `library/never-written.toml`.
const NEVER_WRITTEN: &str = include_str!("../../library/never-written.toml");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NeverWritten {
    names: Vec<String>,
}

static NEVER_WRITTEN_NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
    let list: NeverWritten =
        toml::from_str(NEVER_WRITTEN).expect("library/never-written.toml parses");
    list.names.iter().map(|name| name.to_lowercase()).collect()
});

/// Whether no task writes the relative path: some part of it is named in
/// `library/never-written.toml` (sorcerer's and git's own, and the config an agent, a shell or
/// an editor obeys when it next starts), ignoring case. In the notes (`.sorcerer/notes/`) the
/// `.sorcerer` it starts with does not count: a note is written like any file.
///
/// Example: `.sorcerer/config.toml`, `.git/hooks/pre-commit`, `tools/.envrc` and
/// `.sorcerer/notes/.envrc` are never written; `src/envrc.txt` and `.sorcerer/notes/plan.md`
/// may be.
pub fn is_never_written(relative: &Path) -> bool {
    let sorcerer_part = usize::from(is_in_notes(relative));
    relative.components().skip(sorcerer_part).any(|part| {
        let name = part.as_os_str().to_string_lossy().to_lowercase();
        NEVER_WRITTEN_NAMES.contains(&name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_out_of_the_project_is_none() {
        let root = Path::new("/proj");
        for written in ["../x.txt", "a/../../x", "", ".", "C:x", "a.txt:s"] {
            assert_eq!(project_relative(root, written), None, "{written}");
        }
        assert_eq!(
            project_relative(root, "src/./a.txt"),
            Some(PathBuf::from("src").join("a.txt"))
        );
    }

    #[test]
    fn the_never_written_paths_are_known() {
        for path in [".sorcerer/state/x", ".git/config", "a/.ENVRC", ".mcp.json"] {
            assert!(is_never_written(Path::new(path)), "{path}");
        }
        for path in [
            ".sorcerer/notes/.envrc",
            ".sorcerer/notes2/x.md",
            ".sorcerer",
        ] {
            assert!(is_never_written(Path::new(path)), "{path}");
        }
        for path in [
            "src/a.txt",
            "src/envrc.txt",
            "docs/mcp.json",
            ".sorcerer/notes/plan.md",
        ] {
            assert!(!is_never_written(Path::new(path)), "{path}");
        }
    }
}
