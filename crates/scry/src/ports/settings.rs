//! scry's settings: its `[scry]` table and the project's steering.

use std::path::PathBuf;

use sorcerer_core::Result;

use crate::domain::config::ScryConfig;
use crate::domain::conjure_drops::ConjureDrop;
use crate::domain::steering::Steering;

pub trait Settings: Send + Sync {
    /// The `[scry]` settings, with defaults for the keys left out.
    fn scry_config(&self) -> Result<ScryConfig>;
    /// The project's steering, read and checked.
    fn steering(&self) -> Result<Steering>;
    /// The steering file, when the project has one.
    fn steering_source(&self) -> Option<PathBuf>;
    /// What conjure left for the next review: code a rules retry dropped. None by default, and
    /// none when conjure's file is missing or of another version.
    fn conjure_drops(&self) -> Vec<ConjureDrop> {
        Vec::new()
    }
}
