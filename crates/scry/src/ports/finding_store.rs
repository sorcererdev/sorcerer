//! The finding store: read whole, updated under a lock, and held by a
//! pass from start to end.

use std::path::Path;

use sorcerer_core::Result;
use sorcerer_core::adapters::json_store::Snapshot;

use crate::domain::findings::Findings;

pub trait FindingStore: Send + Sync {
    fn read(&self) -> Result<Snapshot<Findings>>;
    /// Reads, applies F, writes, under the store's lock; an unreadable
    /// store is an error and is left alone.
    fn update(&self, f: &mut dyn FnMut(&mut Findings)) -> Result<()>;
    /// Holds the pass lock until the returned value is dropped: one pass at
    /// a time per project.
    fn hold(&self) -> Result<Box<dyn Held>>;
    fn path(&self) -> &Path;
}

/// A held pass lock.
pub trait Held {}
