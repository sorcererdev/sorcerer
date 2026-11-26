//! The commands allowed always, `.sorcerer/state/apprentice/permissions.json`, in core's
//! locked JSON store.

use sorcerer_core::Result;
use sorcerer_core::adapters::json_store::{Snapshot, Store};

use crate::domain::permissions::CommandPrefixes;
use crate::ports::permission_store::PermissionStore;

pub struct JsonPermissions {
    store: Store<CommandPrefixes>,
}

impl JsonPermissions {
    pub fn at(path: std::path::PathBuf) -> JsonPermissions {
        JsonPermissions {
            store: Store::at(path),
        }
    }
}

impl PermissionStore for JsonPermissions {
    /// A file that cannot be read allows nothing (and is never overwritten).
    fn allowed(&self) -> CommandPrefixes {
        match self.store.read() {
            Ok(Snapshot::Current(prefixes)) => prefixes,
            _ => CommandPrefixes::default(),
        }
    }

    fn allow_always(&self, prefix: &str) -> Result<()> {
        self.store
            .update(|prefixes| *prefixes = prefixes.with(prefix))
    }
}
