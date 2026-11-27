//! The commands the person allowed *always for this project* (`permissions.json`).

use sorcerer_core::Result;

use crate::domain::permissions::AllowedCommands;

pub trait PermissionStore: Send + Sync {
    fn allowed(&self) -> AllowedCommands;
    /// Remembers an exact command line for good.
    fn allow_always(&self, line: &str) -> Result<()>;
}
