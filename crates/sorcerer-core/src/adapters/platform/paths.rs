//! Paths as each platform wants them.

use std::path::{Path, PathBuf};

/// The absolute path with every symlink and `..` resolved, as the platform's own tools write
/// it.
///
/// On Windows the standard library's `canonicalize` answers `\\?\C:\Users\…`, a form most
/// programs (a C compiler, git, an editor) do not accept; this answers `C:\Users\…`. Elsewhere
/// it is the standard `canonicalize`. Example: in a directory reached through the symlink
/// `/tmp`, `canonical(Path::new("/tmp/x"))` on macOS is `/private/tmp/x`.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// Whether the file system holding `dir` treats `Foo.py` and `foo.py` as one name (the default
/// on macOS and Windows, not on Linux). It looks, without writing: the first entry of `dir`
/// with a letter in its name is asked for again with the case of its letters flipped, and the
/// file system answers. A directory with no such entry is taken as case-sensitive.
///
/// Example: in a project root holding `.git`, asking for `.GIT` finds it on macOS and not on
/// Linux.
pub fn names_ignore_case(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|name| name.chars().any(char::is_alphabetic))
        .is_some_and(|name| {
            let flipped: String = name
                .chars()
                .map(|c| {
                    if c.is_lowercase() {
                        c.to_uppercase().collect::<String>()
                    } else {
                        c.to_lowercase().collect::<String>()
                    }
                })
                .collect();
            dir.join(flipped).symlink_metadata().is_ok()
        })
}

/// Makes `link` a symbolic link to `target`, for tests. On Windows a link to a directory and a
/// link to a file are different kinds, so `target` must exist when this is called there.
#[doc(hidden)]
pub fn link_to(target: &Path, link: &Path) -> std::io::Result<()> {
    make_link(target, link)
}

#[cfg(unix)]
fn make_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn make_link(target: &Path, link: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_has_no_verbatim_prefix() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = canonical(dir.path()).unwrap();
        assert!(path.is_absolute(), "{}", path.display());
        assert!(
            !path.to_string_lossy().starts_with(r"\\?\"),
            "{}",
            path.display()
        );
    }

    #[test]
    fn names_ignore_case_agrees_with_the_file_system() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(
            !names_ignore_case(dir.path()),
            "an empty directory has no answer: false"
        );
        std::fs::write(dir.path().join("Probe.txt"), "x").unwrap();
        let flipped_exists = dir.path().join("pROBE.TXT").exists();
        assert_eq!(names_ignore_case(dir.path()), flipped_exists);
    }

    #[test]
    fn link_to_links_a_directory_and_a_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let target_dir = dir.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        std::fs::write(target_dir.join("f.txt"), "hi").unwrap();
        let dir_link = dir.path().join("dir-link");
        link_to(&target_dir, &dir_link).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir_link.join("f.txt")).unwrap(),
            "hi"
        );
        let file_link = dir.path().join("file-link");
        link_to(&target_dir.join("f.txt"), &file_link).unwrap();
        assert_eq!(std::fs::read_to_string(&file_link).unwrap(), "hi");
        assert!(
            file_link
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
