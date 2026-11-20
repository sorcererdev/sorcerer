//! Opening a project from any directory inside it. A repository with no `.sorcerer/` directory
//! is still a project: its git root, with the default settings.

use std::path::{Path, PathBuf};

use crate::domain::config::Config;
use crate::domain::language::Languages;
use crate::domain::project::Project;
use crate::{Error, Result};

impl Project {
    /// Opens the project that a directory is in, looking upward from it.
    ///
    /// The root is the nearest directory holding `.sorcerer/`; when there is none, the nearest
    /// holding `.git`. Example: from `~/shop/src/orders`, with `~/shop/.git` and no
    /// `.sorcerer/` anywhere, the root is `~/shop` with the default settings. With neither
    /// above, it is an error.
    pub fn open(dir: &Path) -> Result<Project> {
        let dir = dir.canonicalize().map_err(|e| Error::io(dir, e))?;
        let root = find_up(&dir, |d| d.join(".sorcerer").is_dir())
            .or_else(|| find_up(&dir, |d| d.join(".git").exists()))
            .ok_or(Error::NoProject { from: dir.clone() })?;
        let config = read_config(&root)?;
        let languages = Languages::assemble(&root, &config.languages)?;
        let state = root.join(".sorcerer/state");
        Ok(Project {
            root,
            config,
            languages,
            state,
        })
    }

    /// The directory where products keep what they remember, `.sorcerer/state`. The first call
    /// creates it with a `.gitignore` holding `*`, so nothing in it is ever committed.
    pub fn state_dir(&self) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.state).map_err(|e| Error::io(&self.state, e))?;
        let ignore = self.state.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n").map_err(|e| Error::io(&ignore, e))?;
        }
        Ok(self.state.clone())
    }
}

/// `.sorcerer/config.toml`, or the default settings when the file is not there.
fn read_config(root: &Path) -> Result<Config> {
    let file = root.join(".sorcerer/config.toml");
    match std::fs::read_to_string(&file) {
        Ok(text) => Config::parse(&text, &file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(Error::io(&file, e)),
    }
}

/// The directory itself or the nearest one above it that passes the test.
fn find_up(dir: &Path, found: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    dir.ancestors().find(|d| found(d)).map(Path::to_path_buf)
}
