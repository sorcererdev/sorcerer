//! Which written paths may land, and a file's new text as line edits against its base text.

use std::path::{Component, Path, PathBuf};

pub use sorcerer_lsp::line_edits::{
    LineEdit, first_changed_line, in_base_line_endings, keep_unterminated_last_line, line_edits,
    lines_changed, same_text,
};

/// Whether the file system the project is on treats `Foo.py` and `foo.py` as one name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameCase {
    /// The names are one (macOS and Windows by default).
    Ignored,
    /// The names are two (Linux by default).
    Matters,
}

/// Whether a path the writer named lands, and if not, why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landing {
    Lands(PathBuf),
    Dropped { path: String, why: DropReason },
}

/// Why a written path was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// It leads out of the project, or is absolute and outside it.
    OutsideProject,
    /// A fence refuses it (`.env`, `.sorcerer/`, `.git/`, `not_evidence`, a link out).
    Fenced,
    /// A `[conjure] exclude` glob matches it.
    Excluded,
    /// It is there but cannot be read as text (a binary file, or a directory), so there is no
    /// text to edit.
    NotText,
}

/// The path the writer named, relative to the project root after lexical clean-up: `.`
/// dropped, `..` taken as far as the root and no further; an absolute path only when it is
/// under `root`. `None` when it is empty, leaves the root or is absolute elsewhere.
///
/// Example: `src/./a.txt` is `src/a.txt`; `a/../../x` and `/etc/passwd` are `None`.
pub fn lexical_relative(root: &Path, written: &str) -> Option<PathBuf> {
    let path = Path::new(written);
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let mut kept: Vec<&std::ffi::OsStr> = Vec::new();
    for part in relative.components() {
        match part {
            // a drive (`C:x`), a stream (`a.txt:s`) or a backslash is not one name on every
            // platform, so it is no name that lands
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_out_of_the_project_is_dropped() {
        // spellbook:allow domain-does-no-io — a test names a project root under the temporary directory
        let root = &std::env::temp_dir().join("proj");
        for written in [
            "../x.txt",
            "a/../../x",
            "",
            ".",
            "./",
            "/etc/passwd",
            "../../etc/passwd",
        ] {
            assert_eq!(lexical_relative(root, written), None, "{written}");
        }
        assert_eq!(
            lexical_relative(root, "src/./a.txt"),
            Some(PathBuf::from("src").join("a.txt"))
        );
        assert_eq!(
            lexical_relative(root, "src/x/../a.txt"),
            Some(PathBuf::from("src").join("a.txt"))
        );
        let inside = root.join("src").join("a.txt");
        assert_eq!(
            lexical_relative(root, &inside.to_string_lossy()),
            Some(PathBuf::from("src").join("a.txt"))
        );
    }

    #[test]
    fn a_name_with_a_drive_or_a_stream_is_not_a_path_that_lands() {
        // spellbook:allow domain-does-no-io — a test names a project root under the temporary directory
        let root = &std::env::temp_dir().join("proj");
        for written in ["C:\\Windows\\x", "a.txt:stream", "C:x"] {
            assert_eq!(lexical_relative(root, written), None, "{written}");
        }
    }
}
