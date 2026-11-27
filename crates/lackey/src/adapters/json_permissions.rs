//! The commands allowed always, `.sorcerer/state/lackey/permissions.json`, in core's
//! locked JSON store. A version 1 file (first words, such as `grep`) is migrated to version 2
//! (exact command lines) the first time it is read: `CommandPrefixes::migrated` keeps each
//! prefix of two words or more as that exact command line and drops bare program names and
//! tree readers.

use sorcerer_core::Result;
use sorcerer_core::adapters::json_store::{Snapshot, Store};

use crate::domain::permissions::{AllowedCommands, CommandPrefixes};
use crate::domain::tree_readers::TreeReaders;
use crate::ports::permission_store::PermissionStore;

pub struct JsonPermissions {
    store: Store<AllowedCommands>,
    version_1: Store<CommandPrefixes>,
}

impl JsonPermissions {
    pub fn at(path: std::path::PathBuf) -> JsonPermissions {
        JsonPermissions {
            store: Store::at(path.clone()),
            version_1: Store::at(path),
        }
    }

    /// Rewrites a version 1 file as version 2; anything else is left as it is.
    ///
    /// Two tasks migrating at once both write the same list, so the race between taking the
    /// old file away and writing the new one loses nothing.
    fn migrate_version_1(&self) -> Result<()> {
        let Ok(Snapshot::Current(old)) = self.version_1.read() else {
            return Ok(());
        };
        let (commands, _dropped) = old.migrated(TreeReaders::shipped());
        std::fs::remove_file(self.store.path())
            .map_err(|e| sorcerer_core::Error::io(self.store.path(), e))?;
        self.store.update(|allowed| *allowed = commands)
    }
}

impl PermissionStore for JsonPermissions {
    /// A file that cannot be read allows nothing (and is never overwritten).
    fn allowed(&self) -> AllowedCommands {
        let _ = self.migrate_version_1();
        match self.store.read() {
            Ok(Snapshot::Current(allowed)) => allowed,
            _ => AllowedCommands::default(),
        }
    }

    fn allow_always(&self, line: &str) -> Result<()> {
        self.migrate_version_1()?;
        self.store.update(|allowed| *allowed = allowed.with(line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_1_file_is_migrated_on_first_read() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("permissions.json");
        std::fs::write(
            &path,
            r#"{"version":1,"data":{"allowed":["just test","grep","git show"]}}"#,
        )
        .unwrap();
        let permissions = JsonPermissions::at(path.clone());
        assert_eq!(permissions.allowed().commands, ["just test"]);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("\"version\": 2"), "{saved}");
        permissions.allow_always("make test").unwrap();
        assert_eq!(permissions.allowed().commands, ["just test", "make test"]);
    }
}
