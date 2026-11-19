//! Scry's settings: its `[scry]` table and the project's steering.

use sorcerer_core::Result;

use crate::domain::config::ScryConfig;
use crate::domain::steering::Steering;

pub trait Settings: Send + Sync {
    fn scry_config(&self) -> Result<ScryConfig>;
    fn steering(&self) -> Result<Steering>;
}
