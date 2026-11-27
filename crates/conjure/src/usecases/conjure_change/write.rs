//! The one call to the writer.

use sorcerer_core::Cancel;

use crate::ports::writer::{Writer, Written};

/// Asks the writer once. A call that fails (a timeout, a refusal, a cancel) is the reason; the
/// writer has already recorded it in the ledger.
pub(super) fn write_once(
    writer: &dyn Writer,
    situation: &str,
    cancel: &Cancel,
) -> Result<Written, String> {
    writer.write(situation, cancel).map_err(|e| e.to_string())
}

/// Asks the writer again after a rules check broke its first answer; the ledger records the call
/// as a retry.
pub(super) fn write_retry(
    writer: &dyn Writer,
    situation: &str,
    cancel: &Cancel,
) -> Result<Written, String> {
    writer
        .write_retry(situation, cancel)
        .map_err(|e| e.to_string())
}
