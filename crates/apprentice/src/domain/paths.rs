//! Which paths a task may name: a path the agent wrote, relative to the project root after
//! lexical clean-up (conjure's rule, copied: products never import each other).

use std::path::{Component, Path, PathBuf};

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

/// Whether a relative path is under sorcerer's own directory, which no task writes.
pub fn is_sorcerers_own(relative: &Path) -> bool {
    relative
        .components()
        .next()
        .is_some_and(|first| first.as_os_str() == ".sorcerer" || first.as_os_str() == ".git")
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
    fn sorcerers_own_directories_are_known() {
        assert!(is_sorcerers_own(Path::new(".sorcerer/state/x")));
        assert!(is_sorcerers_own(Path::new(".git/config")));
        assert!(!is_sorcerers_own(Path::new("src/a.txt")));
    }
}
