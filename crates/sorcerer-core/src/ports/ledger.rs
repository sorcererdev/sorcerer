//! The ledger: one row per paid call (an errand) of every product.
//! Append-only; caps and `cost` commands read it.

use std::path::Path;

use crate::Result;
use crate::domain::errand::{Errand, LedgerRows};

/// Where every paid model call is written down.
pub trait Ledger: Send + Sync {
    /// Adds one call at the end.
    fn append(&self, errand: &Errand) -> Result<()>;
    /// Every call written down so far, and how many lines could not be read.
    fn read(&self) -> Result<LedgerRows>;
    /// Where the rows live.
    fn path(&self) -> &Path;
}
