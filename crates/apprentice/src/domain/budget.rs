//! The caps: what a task may spend, what a day may, what Jev's triage may. Each day is the
//! user's local day, counted by core's `spent_since` over the ledger.

use chrono::{DateTime, FixedOffset, Utc};
use sorcerer_core::domain::cost::spent_since;
use sorcerer_core::domain::errand::Errand;

use crate::domain::config::{ApprenticeConfig, PRODUCT};

/// Jev's errands are recorded with this verb.
pub const TRIAGE_VERB: &str = "decide";

/// What apprentice has spent today, in dollars: every errand of the product since the start of
/// the user's local day.
pub fn spent_today(errands: &[Errand], now: DateTime<Utc>, offset: FixedOffset) -> f64 {
    spent_since(errands, PRODUCT, local_day(now, offset), offset)
}

/// What Jev's triage has spent today, in dollars.
pub fn triage_spent_today(errands: &[Errand], now: DateTime<Utc>, offset: FixedOffset) -> f64 {
    let triage: Vec<Errand> = errands
        .iter()
        .filter(|errand| errand.verb == TRIAGE_VERB)
        .cloned()
        .collect();
    spent_today(&triage, now, offset)
}

/// Whether the day still allows a call: what was spent is under the day's cap.
///
/// Example: with `budget_per_day = 5.00`, $4.99 spent allows one more task, $5.00 does not.
pub fn day_allows(spent: f64, config: &ApprenticeConfig) -> bool {
    spent < config.budget_per_day
}

/// Whether the day still allows a triage call: Jev's spending is under its own cap and the
/// day's.
pub fn triage_allows(triage_spent: f64, spent: f64, config: &ApprenticeConfig) -> bool {
    triage_spent < config.triage_per_day && day_allows(spent, config)
}

fn local_day(now: DateTime<Utc>, offset: FixedOffset) -> chrono::NaiveDate {
    now.with_timezone(&offset).date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errand(verb: &str, cost: f64) -> Errand {
        Errand {
            at: "2026-11-26T15:00:00Z".parse().unwrap(),
            product: PRODUCT.into(),
            verb: verb.into(),
            model: "sonnet".into(),
            provider: "claude".into(),
            cost_usd: Some(cost),
            input_tokens: 0,
            output_tokens: 0,
            seconds: 1.0,
            ok: true,
            note: None,
        }
    }

    #[test]
    fn the_day_and_triage_count_their_own_errands() {
        let now = "2026-11-26T20:00:00Z".parse().unwrap();
        let offset = FixedOffset::west_opt(5 * 3600).unwrap();
        let errands = [errand("task", 1.0), errand(TRIAGE_VERB, 0.05)];
        assert!((spent_today(&errands, now, offset) - 1.05).abs() < 1e-9);
        assert!((triage_spent_today(&errands, now, offset) - 0.05).abs() < 1e-9);
        let config = ApprenticeConfig::default();
        assert!(day_allows(4.99, &config));
        assert!(!day_allows(5.00, &config));
        assert!(triage_allows(0.19, 1.0, &config));
        assert!(!triage_allows(0.20, 1.0, &config));
    }
}
