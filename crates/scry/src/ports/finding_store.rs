//! The findings store: read whole, changed under a lock so two writers
//! never clash, and held by a pass from start to end so two passes never
//! run at once.

use std::path::Path;

use sorcerer_core::Result;
use sorcerer_core::domain::versioned::Snapshot;

use crate::domain::findings::Findings;

pub trait FindingStore: Send + Sync {
    /// The findings as stored now: none yet, the findings, or why they
    /// cannot be read.
    fn read(&self) -> Result<Snapshot<Findings>>;
    /// Reads the findings, changes them with `f`, and writes them back, all
    /// under the store's lock. A store that cannot be read is an error and
    /// is left as it is.
    fn update(&self, f: &mut dyn FnMut(&mut Findings)) -> Result<()>;
    /// Holds the pass lock until the returned value is dropped: one pass at
    /// a time per project.
    fn hold(&self) -> Result<Box<dyn Held>>;
    /// Where the store is, for messages.
    fn path(&self) -> &Path;
}

/// A pass lock being held: dropping it lets the next pass in.
pub trait Held {}
