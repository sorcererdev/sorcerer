//! The writer seam: the model that writes a change. conjure's writer is its own interface over
//! the same providers scry uses.

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;

/// Writes one change on a situation. Every call is recorded in the ledger as one of conjure's
/// errands (`conjure` for the first attempt, `try-harder` for the second), failures too.
pub trait Writer: Send + Sync {
    /// The writer's reply as it came; reading it is the use case's job.
    fn write(&self, situation: &str, cancel: &Cancel) -> sorcerer_models::Result<Written>;
    /// The model alias that writes, as the mark says it.
    fn model(&self) -> &str;
}

/// A writer's reply and the ledger row of the call.
#[derive(Debug, Clone)]
pub struct Written {
    pub text: String,
    pub errand: Errand,
}
