//! Where the records of changes are kept.

use sorcerer_core::Result;

use crate::domain::record::{ChangeRecord, ChangeRows};

pub trait ChangeStore: Send + Sync {
    /// Adds one record at the end.
    fn append(&self, record: &ChangeRecord) -> Result<()>;
    /// Every record written so far; a line that is of another version or does not read is
    /// counted and skipped.
    fn read(&self) -> Result<ChangeRows>;
}
