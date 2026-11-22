//! What conjure spent, a day at a time, for `conjure cost`.

use std::collections::BTreeMap;

use chrono::{FixedOffset, NaiveDate};
use sorcerer_core::ports::ledger::Ledger;

use crate::domain::config::PRODUCT;

/// What conjure spent in one of the user's local days.
#[derive(Debug, Clone, PartialEq)]
pub struct DayCost {
    pub day: NaiveDate,
    /// First attempts (the `conjure` errands).
    pub changes: usize,
    /// Second attempts (the `try-harder` errands).
    pub try_harders: usize,
    /// Every conjure errand of the day, in dollars.
    pub total_cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CostReport {
    pub days: Vec<DayCost>,
}

/// What conjure spent in each of the user's local days (`offset` is their time zone), from the
/// `since` day on when given, oldest first. A call whose cost was not recorded counts $0.
///
/// Example: two first attempts at $0.001 and one Try harder at $0.02 on one day are a row:
/// 2 changes, 1 try harder, $0.022.
pub fn report_cost(
    ledger: &dyn Ledger,
    offset: FixedOffset,
    since: Option<NaiveDate>,
) -> anyhow::Result<CostReport> {
    let errands = ledger.read()?.errands;
    let mut days: BTreeMap<NaiveDate, DayCost> = BTreeMap::new();
    for errand in errands.iter().filter(|errand| errand.product == PRODUCT) {
        let day = errand.at.with_timezone(&offset).date_naive();
        if since.is_some_and(|first| day < first) {
            continue;
        }
        let row = days.entry(day).or_insert(DayCost {
            day,
            changes: 0,
            try_harders: 0,
            total_cost: 0.0,
        });
        match errand.verb.as_str() {
            "conjure" => row.changes += 1,
            "try-harder" => row.try_harders += 1,
            _ => {}
        }
        row.total_cost += errand.cost_usd.unwrap_or(0.0);
    }
    Ok(CostReport {
        days: days.into_values().collect(),
    })
}
