//! The ledger: one row per paid call (an errand) of every product.
//! Append-only; caps and `cost` commands read it.

use std::path::Path;

use crate::Result;
use crate::domain::errand::{Errand, LedgerRows};

pub trait Ledger: Send + Sync {
    /// Appends one row.
    fn append(&self, e: &Errand) -> Result<()>;
    /// Every row, and how many did not parse.
    fn read(&self) -> Result<LedgerRows>;
    /// Where the rows live.
    fn path(&self) -> &Path;
}
