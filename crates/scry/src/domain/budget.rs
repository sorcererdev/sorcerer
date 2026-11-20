//! Spending: what a model call is likely to cost, and whether the pass may
//! still make it without going over any of its spending caps.

use sorcerer_core::domain::errand::Errand;

/// The most a pass may spend, in dollars, under each of three caps.
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    /// The editor's cap on one pass (`budget_per_pass`); none from the
    /// command line.
    pub pass: Option<f64>,
    /// The cap on all of scry's spending in the user's local day
    /// (`budget_per_day`).
    pub day: f64,
    /// The cap the user gave this run (`scry --budget 0.50`), if any.
    pub budget: Option<f64>,
}

/// What a review call on sonnet cost when measured: the guess until the
/// ledger has priced review calls of its own.
pub const REVIEW_DEFAULT: f64 = 0.04;
/// The same for a review call on a haiku model.
pub const REVIEW_HAIKU_DEFAULT: f64 = 0.005;
/// The same for a claim check.
pub const CHECK_DEFAULT: f64 = 0.016;

/// What the next review call is likely to cost: the average of the last 20
/// review calls the ledger has a price for, or, before there are any,
/// $0.04 ($0.005 when the model is a haiku).
///
/// Example: with priced review calls of $0.03 and $0.05 in the ledger, the
/// estimate is $0.04.
pub fn review_estimate(rows: &[Errand], model: &str) -> f64 {
    let default = if model.contains("haiku") {
        REVIEW_HAIKU_DEFAULT
    } else {
        REVIEW_DEFAULT
    };
    mean_or(rows, "review", default)
}

/// What a claim check is likely to cost: the average of the last 20 claim
/// checks the ledger has a price for, or $0.016 before there are any.
pub fn check_estimate(rows: &[Errand]) -> f64 {
    mean_or(rows, "check", CHECK_DEFAULT)
}

/// The average cost of scry's last 20 priced calls of this kind (`review`
/// or `check`), or `default` when there are none.
fn mean_or(rows: &[Errand], verb: &str, default: f64) -> f64 {
    let costs: Vec<f64> = rows
        .iter()
        .rev()
        .filter(|errand| errand.product == "scry" && errand.verb == verb)
        .filter_map(|errand| errand.cost_usd)
        .take(20)
        .collect();
    if costs.is_empty() {
        default
    } else {
        costs.iter().sum::<f64>() / costs.len() as f64
    }
}

/// What a pass has spent, held against its caps. The day's cap counts
/// what scry spent earlier in the day too.
///
/// Example: with a pass cap of $0.25 and a day cap of $3.00, after $2.90
/// spent earlier today and $0.05 in this pass, there is $0.05 left: the
/// day's cap is the nearer.
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    caps: Caps,
    spent_earlier_today: f64,
    spent: f64,
}

impl Budget {
    /// A pass's budget before it spends anything. `spent_today` is what
    /// scry spent earlier in the user's local day.
    pub fn new(caps: &Caps, spent_today: f64) -> Budget {
        Budget {
            caps: caps.clone(),
            spent_earlier_today: spent_today,
            spent: 0.0,
        }
    }

    /// Whether a call likely to cost `estimate` keeps the pass within every
    /// cap.
    pub fn allows(&self, estimate: f64) -> bool {
        let after = self.spent + estimate;
        let over = |cap: Option<f64>| cap.is_some_and(|c| after > c);
        let over_day = self.spent_earlier_today + after > self.caps.day;
        !(over(self.caps.pass) || over_day || over(self.caps.budget))
    }

    /// Adds a call's cost to what the pass has spent.
    pub fn charge(&mut self, cost: f64) {
        self.spent += cost;
    }

    /// What this pass has spent.
    pub fn spent(&self) -> f64 {
        self.spent
    }

    /// How much the pass may still spend: what is left under the nearest
    /// cap, never below 0. None when no cap is finite.
    pub fn money_left(&self) -> Option<f64> {
        let left_today = self.caps.day - self.spent_earlier_today - self.spent;
        [
            self.caps.pass.map(|cap| cap - self.spent),
            Some(left_today).filter(|left| left.is_finite()),
            self.caps.budget.map(|cap| cap - self.spent),
        ]
        .into_iter()
        .flatten()
        .map(|left| left.max(0.0))
        .reduce(f64::min)
    }
}

/// What a model call cost: the sum of its errands' costs, counting an
/// errand with no price as 0.
pub fn cost_of(errands: &[Errand]) -> f64 {
    errands.iter().filter_map(|e| e.cost_usd).sum()
}
