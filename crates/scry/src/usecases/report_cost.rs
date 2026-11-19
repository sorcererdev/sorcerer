//! What scry spent, a local day a row, and what waits on the caps.

use std::collections::BTreeMap;

use chrono::{FixedOffset, NaiveDate};
use sorcerer_core::ports::ledger::Ledger;

use crate::ports::finding_store::FindingStore;
use crate::usecases::read_findings;

#[derive(Debug, Clone, PartialEq)]
pub struct DayCost {
    pub day: NaiveDate,
    pub reviews: usize,
    pub checks: usize,
    pub review_cost: f64,
    pub check_cost: f64,
    /// Every scry errand, Jev's questions too.
    pub total_cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CostReport {
    pub days: Vec<DayCost>,
    /// Units a cap held back; None when the store does not read.
    pub waiting_units: Option<usize>,
    /// Why the store did not read.
    pub store_error: Option<String>,
}

/// Scry's errands in LEDGER by local day (in OFFSET) from SINCE on;
/// `decide` errands count into the total only.
pub fn report_cost(
    ledger: &dyn Ledger,
    store: &dyn FindingStore,
    offset: FixedOffset,
    since: Option<NaiveDate>,
) -> anyhow::Result<CostReport> {
    let rows = ledger.read()?.errands;
    let mut days: BTreeMap<NaiveDate, DayCost> = BTreeMap::new();
    for e in rows.iter().filter(|e| e.product == "scry") {
        let day = e.at.with_timezone(&offset).date_naive();
        if since.is_some_and(|s| day < s) {
            continue;
        }
        let d = days.entry(day).or_insert(DayCost {
            day,
            reviews: 0,
            checks: 0,
            review_cost: 0.0,
            check_cost: 0.0,
            total_cost: 0.0,
        });
        let c = e.cost_usd.unwrap_or(0.0);
        match e.verb.as_str() {
            "review" => {
                d.reviews += 1;
                d.review_cost += c;
            }
            "check" => {
                d.checks += 1;
                d.check_cost += c;
            }
            _ => {}
        }
        d.total_cost += c;
    }
    // a store that does not read is said, not counted as nothing waiting
    let (waiting_units, store_error) = match read_findings(store) {
        Ok(s) => (Some(s.capped.values().map(|u| u.len()).sum()), None),
        Err(e) => (None, Some(e.to_string())),
    };
    Ok(CostReport {
        days: days.into_values().collect(),
        waiting_units,
        store_error,
    })
}
