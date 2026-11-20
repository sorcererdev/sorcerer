//! The researcher seam: a model that reads the project to answer.

use sorcerer_core::Cancel;

use crate::Result;
use crate::domain::research::{Research, Researched};

/// Every researcher records its call in the ledger before it returns,
/// whether the call worked or not, so no spending goes unrecorded. The
/// recorded double is the exception: it spends nothing.
pub trait Researcher: Send + Sync {
    fn research(&self, research: Research, cancel: &Cancel) -> Result<Researched>;
}
