//! Where regions and their answers are kept (`regions.json`), and the labels
//! (`labels.jsonl`).

use sorcerer_core::Result;

use crate::domain::region_record::{Label, RegionRecord};

pub trait RegionStore: Send + Sync {
    fn get(&self, id: &str) -> Option<RegionRecord>;
    /// Every record of a file (relative, `/`).
    fn of_file(&self, file: &str) -> Vec<RegionRecord>;
    /// Keeps the records, replacing any of the same id.
    fn put(&self, records: &[RegionRecord]) -> Result<()>;
    fn label(&self, label: &Label) -> Result<()>;
}
