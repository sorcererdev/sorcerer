//! What scry spent earlier in the day, as the review starts.

use chrono::{DateTime, Utc};
use sorcerer_core::Result;
use sorcerer_core::domain::cost::spent_since;
use sorcerer_core::ports::ledger::Ledger;

use crate::domain::budget::{check_estimate, review_estimate};
use crate::domain::config::ScryConfig;
use crate::ports::clock::Clock;

/// What scry spent earlier in the local day, and what a review call and a
/// claim check are estimated to cost, as of the moment the pass starts.
pub(super) struct SpentSoFar {
    pub(super) as_of: DateTime<Utc>,
    pub(super) today: f64,
    pub(super) review_estimate: f64,
    pub(super) check_estimate: f64,
}

impl SpentSoFar {
    /// Reads the ledger (every paid call scry made) as of the clock's now.
    pub(super) fn read(
        ledger: &dyn Ledger,
        clock: &dyn Clock,
        cfg: &ScryConfig,
    ) -> Result<SpentSoFar> {
        let rows = ledger.read()?.errands;
        let now = clock.now();
        let offset = clock.offset();
        // Jev's questions are not counted against the caps: a fraction of a cent each
        let paid: Vec<_> = rows
            .iter()
            .filter(|e| e.verb == "review" || e.verb == "check")
            .cloned()
            .collect();
        Ok(SpentSoFar {
            as_of: now,
            today: spent_since(
                &paid,
                "scry",
                now.with_timezone(&offset).date_naive(),
                offset,
            ),
            review_estimate: review_estimate(&rows, &cfg.review_model),
            check_estimate: check_estimate(&rows),
        })
    }
}
