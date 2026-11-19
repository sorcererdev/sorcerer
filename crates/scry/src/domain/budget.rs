//! Caps and charges: what a call is likely to cost and whether it still fits
//! the caps (a pass's, the local day's, a run's budget).

use sorcerer_core::domain::errand::Errand;

/// Spend caps in USD: a pass's, the local day's, and a run's budget.
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    pub pass: Option<f64>,
    pub day: f64,
    pub budget: Option<f64>,
}

/// A review call on sonnet, a review call on a haiku model, a claim check:
/// what they cost when measured, used until the ledger has priced rows.
pub const REVIEW_DEFAULT: f64 = 0.04;
pub const REVIEW_HAIKU_DEFAULT: f64 = 0.005;
pub const CHECK_DEFAULT: f64 = 0.016;

/// What the next review call is likely to cost: the mean of the last 20
/// priced `scry` `review` rows, else $0.04 ($0.005 when MODEL is a haiku).
pub fn review_estimate(rows: &[Errand], model: &str) -> f64 {
    let default = if model.contains("haiku") {
        REVIEW_HAIKU_DEFAULT
    } else {
        REVIEW_DEFAULT
    };
    mean_or(rows, "review", default)
}

/// What a claim check is likely to cost: the mean of the last 20 priced
/// `scry` `check` rows, else $0.016.
pub fn check_estimate(rows: &[Errand]) -> f64 {
    mean_or(rows, "check", CHECK_DEFAULT)
}

fn mean_or(rows: &[Errand], verb: &str, default: f64) -> f64 {
    let costs: Vec<f64> = rows
        .iter()
        .rev()
        .filter(|e| e.product == "scry" && e.verb == verb)
        .filter_map(|e| e.cost_usd)
        .take(20)
        .collect();
    if costs.is_empty() {
        default
    } else {
        costs.iter().sum::<f64>() / costs.len() as f64
    }
}

/// A pass's spending against the caps: the pass's own, the local day's
/// (with what scry spent earlier today) and the run's budget.
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    caps: Caps,
    today: f64,
    spent: f64,
}

impl Budget {
    /// SPENT_TODAY is what scry spent earlier in the user's local day.
    pub fn new(caps: &Caps, spent_today: f64) -> Budget {
        Budget {
            caps: caps.clone(),
            today: spent_today,
            spent: 0.0,
        }
    }

    /// Whether a call estimated at E still fits every cap.
    pub fn allows(&self, estimate: f64) -> bool {
        let after = self.spent + estimate;
        let over = |cap: Option<f64>| cap.is_some_and(|c| after > c);
        !(over(self.caps.pass) || self.today + after > self.caps.day || over(self.caps.budget))
    }

    pub fn charge(&mut self, cost: f64) {
        self.spent += cost;
    }

    /// What this pass has spent.
    pub fn spent(&self) -> f64 {
        self.spent
    }

    /// The least room left under any cap, if any cap is finite.
    pub fn room(&self) -> Option<f64> {
        [
            self.caps.pass.map(|c| c - self.spent),
            Some(self.caps.day - self.today - self.spent).filter(|r| r.is_finite()),
            self.caps.budget.map(|c| c - self.spent),
        ]
        .into_iter()
        .flatten()
        .map(|r| r.max(0.0))
        .reduce(f64::min)
    }
}

/// What a paid call is charged: its errands' costs (one with no cost
/// counts zero).
pub fn cost_of(errands: &[Errand]) -> f64 {
    errands.iter().filter_map(|e| e.cost_usd).sum()
}
