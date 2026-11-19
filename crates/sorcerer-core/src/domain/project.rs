//! The project: its root, its config, its languages and its state directory.
//! Opening one reads the disk (`adapters/project.rs`).

use std::path::PathBuf;

use crate::domain::config::Config;
use crate::domain::language::Languages;

#[derive(Debug, Clone)]
pub struct Project {
    /// The directory holding `.sorcerer/`, or the git root when there is none.
    pub root: PathBuf,
    pub config: Config,
    /// The shipped and project language layers, merged.
    pub languages: Languages,
    /// `.sorcerer/state`, created on first write ([`Project::state_dir`]).
    pub state: PathBuf,
}
