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
