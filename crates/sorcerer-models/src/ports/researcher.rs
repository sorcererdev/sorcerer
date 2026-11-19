//! The researcher seam.

use sorcerer_core::Cancel;

use crate::Result;
use crate::domain::research::{Research, Researched};

/// Every implementation appends its errand to the ledger before
/// returning, success or failure (the recorded double excepted: it spends
/// nothing).
pub trait Researcher: Send + Sync {
    fn research(&self, r: Research, cancel: &Cancel) -> Result<Researched>;
}
