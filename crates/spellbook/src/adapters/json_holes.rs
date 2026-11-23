//! The open holes as core's store file, `.sorcerer/state/spellbook/holes.json`
//! (`{"version": 1, "data": {"holes": [...]}}`).

use sorcerer_core::adapters::json_store::{Snapshot, Store};
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::record::{HolesFile, OpenHole};
use crate::ports::holes_store::HolesStore;

/// Where the open holes are kept, under the project's state directory.
pub const HOLES_FILE: &str = "spellbook/holes.json";

pub struct JsonHoles {
    pub store: Store<HolesFile>,
}

impl JsonHoles {
    pub fn for_project(project: &Project) -> Result<JsonHoles> {
        Ok(JsonHoles {
            store: Store::in_project(project, HOLES_FILE)?,
        })
    }
}

impl HolesStore for JsonHoles {
    /// An unreadable file is an error and is left alone.
    fn read(&self) -> Result<Vec<OpenHole>> {
        match self.store.read()? {
            Snapshot::Absent => Ok(Vec::new()),
            Snapshot::Current(file) => Ok(file.holes),
            Snapshot::Unreadable(why) => Err(Error::StoreUnreadable {
                path: self.store.path().to_path_buf(),
                why,
            }),
        }
    }

    fn replace(&self, holes: Vec<OpenHole>) -> Result<()> {
        self.store.update(|file| file.holes = holes)
    }
}
