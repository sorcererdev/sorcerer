//! An editor that keeps buffers in memory, records what the mediums show, and answers questions
//! from a queue. `person_edits` plays the person typing while a task runs.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use apprentice::domain::edits_text::FileEdit;
use apprentice::domain::mediums::{Ask, MarkSeverity};
use apprentice::domain::task::TaskRecord;
use apprentice::ports::editor::{Asked, Editor, Written};
use sorcerer_core::domain::paths::forward_slashes;

#[derive(Default)]
pub struct RecordingEditor {
    pub root: PathBuf,
    /// The open buffers' text, by path relative to the root.
    pub buffers: Mutex<HashMap<PathBuf, String>>,
    looks: Mutex<HashMap<(String, PathBuf), String>>,
    /// Everything shown, as `notify: …`, `mark: file:line …`, `ghost: …`, `note: …`.
    pub shown: Mutex<Vec<String>>,
    pub asked: Mutex<Vec<Ask>>,
    answers: Mutex<VecDeque<Asked>>,
    pub progress: Mutex<Vec<String>>,
    pub finished: Mutex<Vec<(TaskRecord, String)>>,
}

impl RecordingEditor {
    pub fn at(root: &Path) -> RecordingEditor {
        RecordingEditor {
            root: root.to_path_buf(),
            ..RecordingEditor::default()
        }
    }

    /// The answers the person will give, in order; after them, no answer.
    pub fn answering(self, answers: &[&str]) -> RecordingEditor {
        *self.answers.lock().unwrap() = answers
            .iter()
            .map(|answer| Asked::Answered(answer.to_string()))
            .collect();
        self
    }

    /// The person opens a file with this text, or types it.
    pub fn person_edits(&self, path: &str, text: &str) {
        self.buffers
            .lock()
            .unwrap()
            .insert(PathBuf::from(path), text.to_string());
    }

    pub fn buffer(&self, path: &str) -> Option<String> {
        self.buffers.lock().unwrap().get(Path::new(path)).cloned()
    }

    pub fn shown(&self) -> Vec<String> {
        self.shown.lock().unwrap().clone()
    }

    fn show(&self, line: String) {
        self.shown.lock().unwrap().push(line);
    }

    fn text_now(&self, file: &Path) -> Option<String> {
        self.buffers
            .lock()
            .unwrap()
            .get(file)
            .cloned()
            .or_else(|| std::fs::read_to_string(self.root.join(file)).ok())
    }
}

impl Editor for RecordingEditor {
    fn notify(&self, text: &str) {
        self.show(format!("notify: {text}"));
    }

    fn mark(&self, _task: &str, file: &Path, line: u32, text: &str, _severity: MarkSeverity) {
        self.show(format!("mark: {}:{line} {text}", forward_slashes(file)));
    }

    fn ghost(&self, file: &Path, line: u32, lines: &[String]) {
        self.show(format!(
            "ghost: {}:{line} {}",
            forward_slashes(file),
            lines.join("|")
        ));
    }

    fn write_file(&self, task: &str, file: &Path, text: &str) -> Written {
        let now = self.text_now(file);
        let key = (task.to_string(), file.to_path_buf());
        let mut looks = self.looks.lock().unwrap();
        if let (Some(looked), Some(now)) = (looks.get(&key), &now)
            && looked != now
        {
            return Written::MovedOn { now: now.clone() };
        }
        looks.insert(key, text.to_string());
        self.buffers
            .lock()
            .unwrap()
            .insert(file.to_path_buf(), text.to_string());
        Written::Landed { before: now }
    }

    fn note(&self, task: &str, title: &str, text: &str) -> sorcerer_core::Result<PathBuf> {
        self.show(format!("note: {title}: {text}"));
        Ok(self.root.join(format!("{task}-note.md")))
    }

    fn ask(&self, _task: &str, ask: &Ask, _wait: Duration) -> Asked {
        self.asked.lock().unwrap().push(ask.clone());
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Asked::NoAnswer)
    }

    fn edits_since_look(&self, task: &str) -> Vec<FileEdit> {
        let buffers = self.buffers.lock().unwrap().clone();
        let mut looks = self.looks.lock().unwrap();
        looks
            .iter_mut()
            .filter(|((owner, _), _)| owner == task)
            .filter_map(|((_, path), looked)| {
                let now = buffers.get(path)?.clone();
                (now != *looked).then(|| FileEdit {
                    path: forward_slashes(path),
                    before: std::mem::replace(looked, now.clone()),
                    after: now,
                })
            })
            .collect()
    }

    fn progress(&self, _task: &str, text: &str) {
        self.progress.lock().unwrap().push(text.to_string());
    }

    fn finished(&self, task: &TaskRecord, done_text: &str) {
        self.finished
            .lock()
            .unwrap()
            .push((task.clone(), done_text.to_string()));
    }
}
