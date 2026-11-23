//! The generations applied, one record each.

use sorcerer_core::Result;

use crate::domain::record::GenerationRecord;

pub trait GenerationStore: Send + Sync {
    fn append(&self, record: &GenerationRecord) -> Result<()>;
    /// Every record that reads, oldest first.
    fn all(&self) -> Result<Vec<GenerationRecord>>;
}
