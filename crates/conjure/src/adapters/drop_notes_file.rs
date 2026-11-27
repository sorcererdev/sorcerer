//! The drop notes file, `.sorcerer/state/conjure/drops.json`, kept with core's store (locked,
//! written whole).

use chrono::{DateTime, Utc};
use sorcerer_core::Result;
use sorcerer_core::adapters::json_store::Store;
use sorcerer_core::domain::project::Project;

use crate::domain::drop_notes::DropNotes;
use crate::domain::retry_drops::CheckedDrop;

const NOTES_FILE: &str = "conjure/drops.json";

/// Leaves a landed change's said drops for scry; nothing is written when it has none.
pub fn note_drops(
    project: &Project,
    change: &str,
    at: DateTime<Utc>,
    drops: &[CheckedDrop],
) -> Result<()> {
    if !drops.iter().any(|drop| drop.verdict.is_said()) {
        return Ok(());
    }
    Store::<DropNotes>::in_project(project, NOTES_FILE)?
        .update(|notes| *notes = notes.with_change(change, at, drops))
}

/// Takes back an undone or replaced change's notes; a project with no notes file is left so.
pub fn forget_drops(project: &Project, change: &str) -> Result<()> {
    let store = Store::<DropNotes>::in_project(project, NOTES_FILE)?;
    if !store.path().exists() {
        return Ok(());
    }
    store.update(|notes| *notes = notes.without_change(change))
}
