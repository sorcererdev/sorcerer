//! The records as a file, `.sorcerer/state/conjure/changes.jsonl`: one line of JSON a change.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::record::{ChangeRecord, ChangeRows, RECORD_VERSION};
use crate::ports::change_store::ChangeStore;

/// The records kept in one file of JSON lines, appended and never rewritten.
#[derive(Debug, Clone)]
pub struct JsonlChangeStore {
    path: PathBuf,
}

impl JsonlChangeStore {
    /// The project's records, `.sorcerer/state/conjure/changes.jsonl` (the file and its folder
    /// are made when the first record is written).
    pub fn for_project(project: &Project) -> Result<JsonlChangeStore> {
        Ok(JsonlChangeStore {
            path: project.state_dir()?.join("conjure/changes.jsonl"),
        })
    }
}

impl ChangeStore for JsonlChangeStore {
    /// Writes the line in one write call, so lines from two processes never interleave.
    fn append(&self, record: &ChangeRecord) -> Result<()> {
        let mut line = serde_json::to_vec(record).map_err(|e| Error::Config {
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

    fn read(&self) -> Result<ChangeRows> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(rows_of(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ChangeRows::default()),
            Err(e) => Err(Error::io(&self.path, e)),
        }
    }
}

/// A line of another version, or one that does not read, is counted and skipped.
fn rows_of(text: &str) -> ChangeRows {
    let lines = text.lines().filter(|line| !line.trim().is_empty());
    lines.fold(ChangeRows::default(), |mut rows, line| {
        match serde_json::from_str::<ChangeRecord>(line) {
            Ok(record) if record.version == RECORD_VERSION => rows.records.push(record),
            _ => rows.skipped += 1,
        }
        rows
    })
}
