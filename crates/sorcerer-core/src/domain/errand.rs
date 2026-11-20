//! An errand: one paid model call of any product, as the ledger records it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One paid model call, as the ledger records it: who made it, which model, what it cost and
/// how long it took.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Errand {
    /// When the call was made.
    pub at: DateTime<Utc>,
    /// `scry`, `conjure`, …
    pub product: String,
    /// What the call was for within the product: `find`, `check`, `decide`, …
    pub verb: String,
    pub model: String,
    pub provider: String,
    /// Dollars, when the provider reported a cost.
    pub cost_usd: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// How long the call took, in seconds.
    pub seconds: f64,
    /// Whether the call returned an answer (false when it failed or timed out).
    pub ok: bool,
    /// A short free-text remark, such as why the call failed.
    pub note: Option<String>,
}

/// Everything read back from the ledger file.
#[derive(Debug, Default)]
pub struct LedgerRows {
    pub errands: Vec<Errand>,
    /// Lines that did not parse (a crash mid-write, a hand edit).
    pub skipped: usize,
}
