//! What scry spent, a day at a time, for `scry cost`; and how many units
//! wait on the caps.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{FixedOffset, NaiveDate};
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::ports::ledger::Ledger;

use crate::ports::finding_store::FindingStore;
use crate::usecases::read_findings;

/// What scry spent in one of the user's local days, in dollars.
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

/// What `scry cost` reports.
#[derive(Debug, Clone, PartialEq)]
pub struct CostReport {
    pub days: Vec<DayCost>,
    /// How many units a cap kept from review; none when the store could
    /// not be read.
    pub waiting_units: Option<usize>,
    /// Why the store could not be read.
    pub store_error: Option<String>,
}

/// What scry spent in each of the user's local days (`offset` is their
/// time zone), from the `since` day on when given, and how many units wait
/// for the next run because a cap stopped them. Review calls and claim
/// checks are counted and summed apart; Jev's questions count only in the
/// total. A findings store that cannot be read leaves the waiting units
/// unknown and says why, rather than counting nothing as waiting.
///
/// Example: two reviews at $0.04 and one check at $0.016 on 2026-11-20 is
/// one day: 2 reviews for $0.08, 1 check for $0.016, total $0.096 (more if
/// Jev was asked anything).
pub fn report_cost(
    ledger: &dyn Ledger,
    store: &dyn FindingStore,
    offset: FixedOffset,
    since: Option<NaiveDate>,
) -> anyhow::Result<CostReport> {
    let errands = ledger.read()?.errands;
    let local_day = |errand: &Errand| errand.at.with_timezone(&offset).date_naive();
    let days = errands
        .iter()
        .filter(|errand| errand.product == "scry")
        .filter(|errand| since.is_none_or(|first| local_day(errand) >= first))
        .fold(BTreeMap::new(), |mut days, errand| {
            let day = local_day(errand);
            days.entry(day)
                .or_insert_with(|| DayCost::nothing_on(day))
                .add(errand);
            days
        });
    let (waiting_units, store_error) = match read_findings(store) {
        Ok(stored) => (Some(stored.capped.values().map(BTreeSet::len).sum()), None),
        Err(e) => (None, Some(e.to_string())),
    };
    Ok(CostReport {
        days: days.into_values().collect(),
        waiting_units,
        store_error,
    })
}

impl DayCost {
    fn nothing_on(day: NaiveDate) -> DayCost {
        DayCost {
            day,
            reviews: 0,
            checks: 0,
            review_cost: 0.0,
            check_cost: 0.0,
            total_cost: 0.0,
        }
    }

    /// Counts one more errand into the day; one with no price counts $0.
    fn add(&mut self, errand: &Errand) {
        let cost = errand.cost_usd.unwrap_or(0.0);
        match errand.verb.as_str() {
            "review" => {
                self.reviews += 1;
                self.review_cost += cost;
            }
            "check" => {
                self.checks += 1;
                self.check_cost += cost;
            }
            _ => {}
        }
        self.total_cost += cost;
    }
}
