//! scry's settings: its `[scry]` table and the project's steering.

use std::path::PathBuf;

use sorcerer_core::Result;

use crate::domain::config::ScryConfig;
use crate::domain::steering::Steering;

pub trait Settings: Send + Sync {
    /// The `[scry]` settings, with defaults for the keys left out.
    fn scry_config(&self) -> Result<ScryConfig>;
    /// The project's steering, read and checked.
    fn steering(&self) -> Result<Steering>;
    /// The steering file, when the project has one.
    fn steering_source(&self) -> Option<PathBuf>;
}
