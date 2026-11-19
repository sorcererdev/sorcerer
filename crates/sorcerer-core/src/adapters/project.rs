//! Opening a project from any directory inside it; out of the box (no
//! `.sorcerer/`) it is the git root with the default config.

use std::path::{Path, PathBuf};

use crate::domain::config::Config;
use crate::domain::language::Languages;
use crate::domain::project::Project;
use crate::{Error, Result};

impl Project {
    /// Walks up from DIR: the first directory holding `.sorcerer/` is the
    /// root and its `config.toml` (if any) is read; else the first holding
    /// `.git` is the root with the default config.
    pub fn open(dir: &Path) -> Result<Project> {
        let dir = dir.canonicalize().map_err(|e| Error::io(dir, e))?;
        let root = find_up(&dir, |d| d.join(".sorcerer").is_dir())
            .or_else(|| find_up(&dir, |d| d.join(".git").exists()))
            .ok_or(Error::NoProject { from: dir.clone() })?;
        let file = root.join(".sorcerer/config.toml");
        let config = match std::fs::read_to_string(&file) {
            Ok(text) => Config::parse(&text, &file)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(Error::io(&file, e)),
        };
        let languages = Languages::assemble(&root, &config.languages)?;
        let state = root.join(".sorcerer/state");
        Ok(Project {
            root,
            config,
            languages,
            state,
        })
    }

    /// The state directory, created on first call with a `.gitignore` of
    /// `*` so nothing in it is ever committed.
    pub fn state_dir(&self) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.state).map_err(|e| Error::io(&self.state, e))?;
        let ignore = self.state.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n").map_err(|e| Error::io(&ignore, e))?;
        }
        Ok(self.state.clone())
    }
}

fn find_up(dir: &Path, found: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    dir.ancestors().find(|d| found(d)).map(Path::to_path_buf)
}
