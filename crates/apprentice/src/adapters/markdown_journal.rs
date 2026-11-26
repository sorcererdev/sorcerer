//! The journals as files: `.sorcerer/state/apprentice/tasks/<id>/journal.md`, and each changed
//! file's text before the task under `tasks/<id>/before/<path>` (a file the task created is
//! listed in `tasks/<id>/created.txt` instead).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use sorcerer_core::{Error, Result};

use crate::ports::journal::Journal;

pub struct MarkdownJournal {
    /// `.sorcerer/state/apprentice/tasks`.
    dir: PathBuf,
}

impl MarkdownJournal {
    pub fn at(dir: PathBuf) -> MarkdownJournal {
        MarkdownJournal { dir }
    }

    fn task_dir(&self, task: &str) -> PathBuf {
        self.dir.join(task)
    }

    fn created_list(&self, task: &str) -> PathBuf {
        self.task_dir(task).join("created.txt")
    }

    fn before_path(&self, task: &str, path: &str) -> PathBuf {
        path.split('/')
            .fold(self.task_dir(task).join("before"), |dir, part| {
                dir.join(part)
            })
    }

    fn created(&self, task: &str) -> Vec<String> {
        std::fs::read_to_string(self.created_list(task))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

impl Journal for MarkdownJournal {
    fn write(&self, task: &str, heading: &str, text: &str) -> Result<()> {
        let entry = format!("## {heading}\n\n{}\n\n", text.trim_end());
        append(&self.path(task), &entry)
    }

    fn keep_before(&self, task: &str, path: &str, before: Option<&str>) -> Result<()> {
        if self.before(task, path).is_some() {
            return Ok(());
        }
        match before {
            Some(text) => write_making_folders(&self.before_path(task, path), text),
            None => append(&self.created_list(task), &format!("{path}\n")),
        }
    }

    fn before(&self, task: &str, path: &str) -> Option<Option<String>> {
        if self.created(task).iter().any(|created| created == path) {
            return Some(None);
        }
        std::fs::read_to_string(self.before_path(task, path))
            .ok()
            .map(Some)
    }

    fn path(&self, task: &str) -> PathBuf {
        self.task_dir(task).join("journal.md")
    }
}

fn append(path: &Path, text: &str) -> Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| Error::io(folder, e))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| Error::io(path, e))?;
    file.write_all(text.as_bytes())
        .map_err(|e| Error::io(path, e))
}

fn write_making_folders(path: &Path, text: &str) -> Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| Error::io(folder, e))?;
    }
    std::fs::write(path, text).map_err(|e| Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_before_text_is_kept_and_a_created_file_is_known() {
        let dir = tempfile::TempDir::new().unwrap();
        let journal = MarkdownJournal::at(dir.path().to_path_buf());
        journal
            .keep_before("t1", "src/a.txt", Some("one\n"))
            .unwrap();
        journal
            .keep_before("t1", "src/a.txt", Some("two\n"))
            .unwrap();
        journal.keep_before("t1", "src/new.txt", None).unwrap();
        assert_eq!(
            journal.before("t1", "src/a.txt"),
            Some(Some("one\n".into()))
        );
        assert_eq!(journal.before("t1", "src/new.txt"), Some(None));
        assert_eq!(journal.before("t1", "src/other.txt"), None);
        journal.write("t1", "notify", "done").unwrap();
        let text = std::fs::read_to_string(journal.path("t1")).unwrap();
        assert_eq!(text, "## notify\n\ndone\n\n");
    }
}
