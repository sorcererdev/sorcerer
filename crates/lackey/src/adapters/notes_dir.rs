//! Where notes live: `.sorcerer/notes/`, the one directory under `.sorcerer/` that the fences
//! leave open (`sorcerer_core::domain::fences::NOTES_DIR`). A note is an ordinary file there:
//! the person can write in it, and lackey triages it, answers it on save and takes
//! *do this* from it like any other file.
//!
//! Two kinds are written today, each named so that a later one never takes its place: a task's
//! note (`t3-why-is-it-slow.md`, the `note` medium) and a long answer kept for its region
//! (`r12.md`). A new kind is one more method naming its file the same way.

use std::io::Write;
use std::path::{Path, PathBuf};

use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::domain::fences::NOTES_DIR;
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

/// A project's notes directory.
#[derive(Debug, Clone)]
pub struct NotesDir {
    root: PathBuf,
    dir: PathBuf,
}

/// What moving the notes from their old place did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotesMoved {
    pub moved: usize,
    /// Notes left in the old place because a note of the same name is already in the new one.
    pub left: Vec<PathBuf>,
}

impl NotesDir {
    /// The notes directory of the project (not made until a note is written).
    pub fn for_project(project: &Project) -> NotesDir {
        NotesDir::under(&project.root)
    }

    /// The notes directory under a project root.
    pub fn under(root: &Path) -> NotesDir {
        NotesDir {
            root: root.to_path_buf(),
            dir: root.join(NOTES_DIR),
        }
    }

    /// The directory itself.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A task's note: `<task>-<title as words>.md`, headed by the title.
    ///
    /// Example: task `t3`, title `Why is it slow?` writes `.sorcerer/notes/t3-why-is-it-slow.md`.
    pub fn write_task_note(&self, task: &str, title: &str, text: &str) -> Result<PathBuf> {
        self.write(&format!("{task}-{}.md", slug_of(title)), title, text)
    }

    /// A long answer kept for its region: `<region>.md`, headed by the question.
    pub fn write_answer(&self, region: &str, title: &str, text: &str) -> Result<PathBuf> {
        self.write(&format!("{region}.md"), title, text)
    }

    /// Writes `# <title>`, a blank line and the text to the note `name`, made new: a link that
    /// stands where the note goes is replaced, never written through. The directory must be the
    /// project's own (`.sorcerer/` and `.sorcerer/notes/` not links out of it or into
    /// sorcerer's state), else nothing is written.
    fn write(&self, name: &str, title: &str, text: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.dir).map_err(|e| Error::io(&self.dir, e))?;
        self.refuse_a_borrowed_directory()?;
        let path = self.dir.join(name);
        let _ = std::fs::remove_file(&path);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut file| write!(file, "# {}\n\n{text}\n", title.trim()))
            .map_err(|e| Error::io(&path, e))?;
        Ok(path)
    }

    /// An error unless the directory resolves to `<root>/.sorcerer/notes` itself.
    fn refuse_a_borrowed_directory(&self) -> Result<()> {
        let own = canonical(&self.root)
            .map(|root| root.join(NOTES_DIR))
            .map_err(|e| Error::io(&self.root, e))?;
        let resolved = canonical(&self.dir).map_err(|e| Error::io(&self.dir, e))?;
        if resolved == own {
            return Ok(());
        }
        Err(Error::io(
            &self.dir,
            std::io::Error::other(format!(
                "{NOTES_DIR} leads to {}; notes are written only in the project's own",
                resolved.display()
            )),
        ))
    }

    /// Moves the notes kept in `old` (before 0.3 they were under
    /// `.sorcerer/state/lackey/notes/`) into this directory, then removes `old` when it is
    /// empty. Only plain files move, never a link; a note whose name is already taken here stays
    /// where it was. Nothing happens when `old` is not a directory of its own.
    ///
    /// Example: `state/lackey/notes/t1-why.md` becomes `.sorcerer/notes/t1-why.md`.
    pub fn move_from(&self, old: &Path) -> NotesMoved {
        let is_own_directory = std::fs::symlink_metadata(old).is_ok_and(|meta| meta.is_dir());
        if !is_own_directory || std::fs::create_dir_all(&self.dir).is_err() {
            return NotesMoved::default();
        }
        if self.refuse_a_borrowed_directory().is_err() {
            return NotesMoved::default();
        }
        let moved = std::fs::read_dir(old)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .fold(NotesMoved::default(), |mut so_far, entry| {
                let to = self.dir.join(entry.file_name());
                let free = std::fs::symlink_metadata(&to).is_err();
                if free && std::fs::rename(entry.path(), &to).is_ok() {
                    so_far.moved += 1;
                } else {
                    so_far.left.push(entry.path());
                }
                so_far
            });
        let _ = std::fs::remove_dir(old);
        moved
    }
}

/// A title as a file name: lowercase words joined by `-`, at most 40 characters.
///
/// Example: `Late cancellations: research` is `late-cancellations-research`.
pub fn slug_of(title: &str) -> String {
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let slug: String = words.join("-").chars().take(40).collect();
    if slug.is_empty() { "note".into() } else { slug }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes_in(dir: &Path) -> NotesDir {
        NotesDir::under(&canonical(dir).unwrap())
    }

    #[test]
    fn a_task_note_is_written_in_the_notes_directory() {
        let d = tempfile::TempDir::new().unwrap();
        let path = notes_in(d.path())
            .write_task_note("t3", "Why is it slow?", "It reads twice.")
            .unwrap();
        assert!(
            path.ends_with(".sorcerer/notes/t3-why-is-it-slow.md"),
            "{path:?}"
        );
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "# Why is it slow?\n\nIt reads twice.\n"
        );
    }

    #[test]
    fn old_notes_move_once_and_a_taken_name_stays_behind() {
        let d = tempfile::TempDir::new().unwrap();
        let old = d.path().join(".sorcerer/state/lackey/notes");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("t1-why.md"), "old\n").unwrap();
        std::fs::write(old.join("t2-how.md"), "old two\n").unwrap();
        let notes = notes_in(d.path());
        std::fs::create_dir_all(notes.dir()).unwrap();
        std::fs::write(notes.dir().join("t2-how.md"), "new two\n").unwrap();
        let moved = notes.move_from(&old);
        assert_eq!(moved.moved, 1);
        assert_eq!(moved.left, vec![old.join("t2-how.md")]);
        assert_eq!(
            std::fs::read_to_string(notes.dir().join("t1-why.md")).unwrap(),
            "old\n"
        );
        assert_eq!(
            std::fs::read_to_string(notes.dir().join("t2-how.md")).unwrap(),
            "new two\n"
        );
        let again = notes.move_from(&old);
        assert_eq!(again.moved, 0);
        assert_eq!(again.left, vec![old.join("t2-how.md")]);
        std::fs::remove_file(old.join("t2-how.md")).unwrap();
        assert_eq!(notes.move_from(&old), NotesMoved::default());
        assert!(!old.exists(), "an emptied old place is removed");
    }

    #[test]
    fn a_notes_directory_that_leads_elsewhere_is_not_written_in() {
        use sorcerer_core::adapters::platform::paths::link_to;
        let d = tempfile::TempDir::new().unwrap();
        let elsewhere = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(d.path().join(".sorcerer")).unwrap();
        link_to(elsewhere.path(), &d.path().join(".sorcerer/notes")).unwrap();
        let notes = notes_in(d.path());
        assert!(notes.write_task_note("t1", "Why", "x").is_err());
        assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_link_where_a_note_goes_is_replaced_not_written_through() {
        use sorcerer_core::adapters::platform::paths::link_to;
        let d = tempfile::TempDir::new().unwrap();
        let elsewhere = tempfile::TempDir::new().unwrap();
        let target = elsewhere.path().join("rc");
        std::fs::write(&target, "keep me\n").unwrap();
        let notes = notes_in(d.path());
        std::fs::create_dir_all(notes.dir()).unwrap();
        link_to(&target, &notes.dir().join("r1.md")).unwrap();
        let path = notes.write_answer("r1", "Why?", "Because.").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "# Why?\n\nBecause.\n"
        );
    }

    #[test]
    fn slugs_are_lowercase_words() {
        assert_eq!(
            slug_of("Late cancellations: research"),
            "late-cancellations-research"
        );
        assert_eq!(slug_of("???"), "note");
    }
}
