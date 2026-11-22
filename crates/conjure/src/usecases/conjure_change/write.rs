//! The one call to the writer.

use sorcerer_core::Cancel;

use super::ChangeRefused;
use crate::ports::writer::{Writer, Written};

/// Asks the writer once. A call that fails (a timeout, a refusal, a cancel) is `Failed` with the
/// reason; the writer has already recorded it in the ledger.
pub(super) fn write_once(
    writer: &dyn Writer,
    situation: &str,
    cancel: &Cancel,
) -> Result<Written, ChangeRefused> {
    writer
        .write(situation, cancel)
        .map_err(|e| ChangeRefused::Failed(e.to_string()))
}
