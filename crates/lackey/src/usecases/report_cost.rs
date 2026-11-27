//! What lackey spent, a day at a time, for `lackey cost`.

use std::collections::BTreeMap;

use chrono::{FixedOffset, NaiveDate};
use sorcerer_core::ports::ledger::Ledger;

use crate::domain::config::PRODUCT;

/// What lackey spent in one of the user's local days, and on what: each verb (`decide`
/// for Jev's triage, `answer`, `check`, `task`) with its count and dollars.
#[derive(Debug, Clone, PartialEq)]
pub struct DayCost {
    pub day: NaiveDate,
    /// Each verb's calls and dollars, by verb.
    pub verbs: BTreeMap<String, (usize, f64)>,
    pub total_cost: f64,
}

/// What lackey spent in each of the user's local days (`offset` is their time zone), from
/// the `since` day on when given, oldest first. A call whose cost was not recorded counts $0.
///
/// Example: one task at $0.71 and two triage calls at $0.002 on one day are a row: task 1
/// ($0.71), decide 2 ($0.004), $0.714 in all.
pub fn report_cost(
    ledger: &dyn Ledger,
    offset: FixedOffset,
    since: Option<NaiveDate>,
) -> anyhow::Result<Vec<DayCost>> {
    let errands = ledger.read()?.errands;
    let mut days: BTreeMap<NaiveDate, DayCost> = BTreeMap::new();
    for errand in errands.iter().filter(|errand| errand.product == PRODUCT) {
        let day = errand.at.with_timezone(&offset).date_naive();
        if since.is_some_and(|first| day < first) {
            continue;
        }
        let cost = errand.cost_usd.unwrap_or(0.0);
        let row = days.entry(day).or_insert(DayCost {
            day,
            verbs: BTreeMap::new(),
            total_cost: 0.0,
        });
        let verb = row.verbs.entry(errand.verb.clone()).or_insert((0, 0.0));
        verb.0 += 1;
        verb.1 += cost;
        row.total_cost += cost;
    }
    Ok(days.into_values().collect())
}
