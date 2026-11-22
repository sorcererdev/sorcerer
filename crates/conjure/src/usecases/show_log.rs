//! The last changes recorded, newest first, for `conjure log`.

use sorcerer_core::Result;

use crate::domain::record::ChangeRecord;
use crate::ports::change_store::ChangeStore;

/// The last `n` records, the newest first.
pub fn last_changes(store: &dyn ChangeStore, n: usize) -> Result<Vec<ChangeRecord>> {
    let rows = store.read()?;
    Ok(rows.records.into_iter().rev().take(n).collect())
}
