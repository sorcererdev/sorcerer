//! A project's `.sorcerer/` folder on disk, for installing a pack into it.

use std::path::PathBuf;

use crate::domain::pack::PackStop;
use crate::ports::pack_source::SorcererDir;

/// The `.sorcerer/` folder under a project's root.
pub struct DiskSorcererDir {
    pub root: PathBuf,
}

impl DiskSorcererDir {
    fn full(&self, path: &str) -> PathBuf {
        path.split('/')
            .fold(self.root.join(".sorcerer"), |full, part| full.join(part))
    }
}

impl SorcererDir for DiskSorcererDir {
    fn holds(&self, path: &str) -> bool {
        self.full(path).exists()
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), PackStop> {
        let full = self.full(path);
        let refused = |e: std::io::Error| PackStop::Disk(format!(".sorcerer/{path}: {e}"));
        if let Some(folder) = full.parent() {
            std::fs::create_dir_all(folder).map_err(refused)?;
        }
        std::fs::write(&full, bytes).map_err(refused)
    }
}
