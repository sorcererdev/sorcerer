//! The tasks as a file, `.sorcerer/state/apprentice/tasks.jsonl`: one line of JSON each time a
//! task changes state, appended and never rewritten.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::task::{TASK_RECORD_VERSION, TaskRecord, TaskRows, latest_of_each};
use crate::ports::task_store::TaskStore;

#[derive(Debug, Clone)]
pub struct JsonlTaskStore {
    path: PathBuf,
}

impl JsonlTaskStore {
    /// The project's tasks (the file and its folder are made at the first record).
    pub fn for_project(project: &Project) -> Result<JsonlTaskStore> {
        Ok(JsonlTaskStore::at(
            project.state_dir()?.join("apprentice/tasks.jsonl"),
        ))
    }

    pub fn at(path: PathBuf) -> JsonlTaskStore {
        JsonlTaskStore { path }
    }
}

impl TaskStore for JsonlTaskStore {
    /// Writes the line in one write call, so lines from two processes never interleave.
    fn append(&self, task: &TaskRecord) -> Result<()> {
        let mut line = serde_json::to_vec(task).map_err(|e| Error::Config {
            file: self.path.clone(),
            why: e.to_string(),
        })?;
        line.push(b'\n');
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder).map_err(|e| Error::io(folder, e))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| Error::io(&self.path, e))?;
        file.write_all(&line).map_err(|e| Error::io(&self.path, e))
    }

    fn read(&self) -> Result<TaskRows> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(rows_of(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TaskRows::default()),
            Err(e) => Err(Error::io(&self.path, e)),
        }
    }
}

/// A line of another version, or one that does not read, is counted and skipped.
fn rows_of(text: &str) -> TaskRows {
    let (records, skipped) = text.lines().filter(|line| !line.trim().is_empty()).fold(
        (Vec::new(), 0),
        |(mut records, skipped), line| match serde_json::from_str::<TaskRecord>(line) {
            Ok(record) if record.version == TASK_RECORD_VERSION => {
                records.push(record);
                (records, skipped)
            }
            _ => (records, skipped + 1),
        },
    );
    TaskRows {
        tasks: latest_of_each(records),
        skipped,
    }
}
