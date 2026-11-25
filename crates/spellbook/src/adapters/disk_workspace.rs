//! The workspace on disk: new files only, inside the project, off the fences.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use sorcerer_core::domain::fences::Fences;

use crate::ports::workspace::{Workspace, WriteRefused};

pub struct DiskWorkspace {
    pub root: PathBuf,
    pub fences: Fences,
}

impl Workspace for DiskWorkspace {
    /// Refuses a path the fences do not allow once its links are followed, creates the folders,
    /// and opens the file only if it is not there yet, so a file that appeared after the plan
    /// is never overwritten.
    fn write_new(&self, path: &Path, text: &str) -> Result<(), WriteRefused> {
        let full = self.root.join(path);
        if !self.fences.allows(&full) {
            return Err(WriteRefused::Fenced);
        }
        if let Some(folder) = full.parent() {
            std::fs::create_dir_all(folder).map_err(|e| WriteRefused::Disk(e.to_string()))?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&full)
            .map_err(|e| match e.kind() {
                ErrorKind::AlreadyExists => WriteRefused::Exists,
                _ => WriteRefused::Disk(e.to_string()),
            })?;
        file.write_all(text.as_bytes())
            .map_err(|e| WriteRefused::Disk(e.to_string()))
    }

    fn write_changed(&self, path: &Path, before: &str, after: &str) -> Result<(), WriteRefused> {
        let full = self.root.join(path);
        if !self.fences.allows(&full) {
            return Err(WriteRefused::Fenced);
        }
        match std::fs::read_to_string(&full) {
            Ok(now) if now == before => {}
            _ => return Err(WriteRefused::Changed),
        }
        std::fs::write(&full, after).map_err(|e| WriteRefused::Disk(e.to_string()))
    }
}
