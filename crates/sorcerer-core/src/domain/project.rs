//! The project: its root, its config, its languages and its state directory.
//! Opening one reads the disk (`adapters/project.rs`).

use std::path::PathBuf;

use crate::domain::config::Config;
use crate::domain::language::Languages;

/// A project sorcerer works in: where it is, how it is configured and which languages it knows.
#[derive(Debug, Clone)]
pub struct Project {
    /// The directory holding `.sorcerer/`, or the git root when there is none.
    pub root: PathBuf,
    /// What `.sorcerer/config.toml` says (defaults when there is no file).
    pub config: Config,
    /// The languages sorcerer ships with, with the project's own language files laid on top.
    pub languages: Languages,
    /// Where products keep what they remember between runs: `.sorcerer/state`. The
    /// directory is created the first time something is saved there.
    pub state: PathBuf,
}
