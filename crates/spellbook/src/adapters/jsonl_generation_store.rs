//! The generation records as a file, `.sorcerer/state/spellbook/generations.jsonl`: one line of
//! JSON a generation, appended and never rewritten.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::record::{GenerationRecord, RECORD_VERSION};
use crate::ports::generation_store::GenerationStore;

#[derive(Debug, Clone)]
pub struct JsonlGenerationStore {
    pub path: PathBuf,
}

impl JsonlGenerationStore {
    /// The project's records (the file and its folder are made when the first is written).
    pub fn for_project(project: &Project) -> Result<JsonlGenerationStore> {
        Ok(JsonlGenerationStore {
            path: project.state_dir()?.join("spellbook/generations.jsonl"),
        })
    }
}

impl GenerationStore for JsonlGenerationStore {
    /// Writes the line in one write call, so lines from two processes never interleave.
    fn append(&self, record: &GenerationRecord) -> Result<()> {
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

    /// A line of another version, or one that does not read, is skipped.
    fn all(&self) -> Result<Vec<GenerationRecord>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text
                .lines()
                .filter_map(|line| serde_json::from_str::<GenerationRecord>(line).ok())
                .filter(|record| record.version == RECORD_VERSION)
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(Error::io(&self.path, e)),
        }
    }
}
