//! The writer seam: the model that writes a change. conjure's writer is its own interface over
//! the same providers scry uses.

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;

/// Writes one change on a situation. Every call is recorded in the ledger as one of conjure's
/// errands (`conjure` for the first attempt, `try-harder` for the second, `conjure-retry` for the
/// call after a rules check), failures too.
pub trait Writer: Send + Sync {
    /// The writer's reply as it came; reading it is the use case's job.
    fn write(&self, situation: &str, cancel: &Cancel) -> sorcerer_models::Result<Written>;
    /// Writes again after a rules check broke the first answer: the same call, recorded in the
    /// ledger as a `conjure-retry` errand, so a retry is not counted as a first attempt. A
    /// writer that does not tell the two apart records it as it records every call.
    fn write_retry(&self, situation: &str, cancel: &Cancel) -> sorcerer_models::Result<Written> {
        self.write(situation, cancel)
    }

    /// The model alias that writes, as the mark says it.
    fn model(&self) -> &str;
}

/// A writer's reply and the ledger row of the call.
#[derive(Debug, Clone)]
pub struct Written {
    pub text: String,
    pub errand: Errand,
}
