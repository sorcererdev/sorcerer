//! The day's cap on what conjure may spend.

use crate::domain::record::Attempt;

/// Try harder spends at most 25 cents; it is not started when the day could not pay that.
pub const TRY_HARDER_CAP_USD: f64 = 0.25;

/// Whether a call of this attempt may be made, given what conjure spent today.
///
/// A first call needs the day not spent (`spent_today < budget_per_day`). Try harder needs that
/// too, and room for its own cap: with a $3.00 budget and $2.80 spent it is refused, since it
/// may spend $0.25.
pub fn day_allows(spent_today: f64, budget_per_day: f64, attempt: Attempt) -> bool {
    let room_for_the_call = match attempt {
        Attempt::First => 0.0,
        Attempt::TryHarder => TRY_HARDER_CAP_USD,
    };
    spent_today < budget_per_day && spent_today + room_for_the_call <= budget_per_day
}
