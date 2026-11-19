//! An errand: one paid model call of any product, as the ledger records it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One paid model call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Errand {
    pub at: DateTime<Utc>,
    /// `scry`, `conjure`, …
    pub product: String,
    /// `find`, `check`, `decide`, …
    pub verb: String,
    pub model: String,
    pub provider: String,
    pub cost_usd: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub seconds: f64,
    pub ok: bool,
    pub note: Option<String>,
}

#[derive(Debug, Default)]
pub struct LedgerRows {
    pub errands: Vec<Errand>,
    /// Lines that did not parse (a crash mid-write, a hand edit).
    pub skipped: usize,
}
