//! The editor of `apprentice FILE:LINE "…"`, a task from the terminal: changes are written to
//! the disk, what the agent says is collected to be printed, and a question takes its default
//! (else its first option), so a command is never allowed from the terminal. It exists so the
//! harness can be tried without an editor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use sorcerer_core::Result;
use sorcerer_core::domain::paths::forward_slashes;

use crate::adapters::notes_dir::NotesDir;
use crate::domain::edits_text::FileEdit;
use crate::domain::mediums::{Ask, MarkSeverity};
use crate::domain::task::TaskRecord;
use crate::ports::editor::{Asked, Editor, Written};

pub struct DiskEditor {
    root: PathBuf,
    notes: NotesDir,
    /// What was said, in order, for the command line to print.
    said: Arc<Mutex<Vec<String>>>,
    /// Each task's last look at each file it wrote: the text it wrote.
    looks: Mutex<HashMap<String, HashMap<PathBuf, String>>>,
}

impl DiskEditor {
    /// An editor over the project root; notes go in `notes`.
    pub fn new(root: PathBuf, notes: NotesDir) -> DiskEditor {
        DiskEditor {
            root,
            notes,
            said: Arc::new(Mutex::new(Vec::new())),
            looks: Mutex::new(HashMap::new()),
        }
    }

    /// Everything said so far, taken.
    pub fn take_said(&self) -> Vec<String> {
        std::mem::take(&mut *lock(&self.said))
    }

    fn say(&self, line: String) {
        lock(&self.said).push(line);
    }
}

impl Editor for DiskEditor {
    fn notify(&self, text: &str) {
        self.say(text.to_string());
    }

    fn mark(&self, _task: &str, file: &Path, line: u32, text: &str, _severity: MarkSeverity) {
        self.say(format!("{}:{line}: {text}", forward_slashes(file)));
    }

    fn ghost(&self, file: &Path, line: u32, lines: &[String]) {
        self.say(format!(
            "{}:{line}: suggested: {}",
            forward_slashes(file),
            lines.join(" / ")
        ));
    }

    fn write_file(&self, task: &str, file: &Path, text: &str) -> Written {
        let path = self.root.join(file);
        let before = std::fs::read_to_string(&path).ok();
        let mut looks = lock(&self.looks);
        let looked = looks.entry(task.to_string()).or_default();
        if let (Some(looked), Some(now)) = (looked.get(file), &before)
            && looked != now
        {
            return Written::MovedOn { now: now.clone() };
        }
        if let Some(folder) = path.parent()
            && let Err(e) = std::fs::create_dir_all(folder)
        {
            return Written::Refused(e.to_string());
        }
        if let Err(e) = std::fs::write(&path, text) {
            return Written::Refused(e.to_string());
        }
        looked.insert(file.to_path_buf(), text.to_string());
        drop(looks);
        self.say(format!("wrote {}", forward_slashes(file)));
        Written::Landed { before }
    }

    fn note(&self, task: &str, title: &str, text: &str) -> Result<PathBuf> {
        let path = self.notes.write_task_note(task, title, text)?;
        self.say(format!("note: {}", path.display()));
        Ok(path)
    }

    fn ask(&self, _task: &str, ask: &Ask, _wait: Duration) -> Asked {
        let answer = ask
            .default
            .clone()
            .unwrap_or_else(|| ask.options[0].clone());
        self.say(format!(
            "asked: {} [{}] → {answer}",
            ask.question,
            ask.options.join(" | ")
        ));
        Asked::Answered(answer)
    }

    fn edits_since_look(&self, task: &str) -> Vec<FileEdit> {
        let mut looks = lock(&self.looks);
        let Some(looked) = looks.get_mut(task) else {
            return Vec::new();
        };
        looked
            .iter_mut()
            .filter_map(|(file, before)| {
                let now = std::fs::read_to_string(self.root.join(file)).unwrap_or_default();
                (now != *before).then(|| FileEdit {
                    path: forward_slashes(file),
                    before: std::mem::replace(before, now.clone()),
                    after: now,
                })
            })
            .collect()
    }

    fn progress(&self, _task: &str, _text: &str) {}

    fn finished(&self, _task: &TaskRecord, done_text: &str) {
        self.say(done_text.to_string());
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
