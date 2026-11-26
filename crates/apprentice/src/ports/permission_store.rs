//! The commands the person allowed *always for this project* (`permissions.json`).

use sorcerer_core::Result;

use crate::domain::permissions::CommandPrefixes;

pub trait PermissionStore: Send + Sync {
    fn allowed(&self) -> CommandPrefixes;
    /// Remembers a prefix for good.
    fn allow_always(&self, prefix: &str) -> Result<()>;
}
