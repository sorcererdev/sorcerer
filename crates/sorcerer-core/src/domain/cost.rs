//! What a product has spent, from the ledger. Every cap reads this.

use chrono::{FixedOffset, NaiveDate};

use crate::domain::errand::Errand;

/// How many dollars one product (such as `"scry"`) has spent, counting its calls in the ledger
/// from the start of a given day until now.
///
/// The day is the user's local day: `offset` is their time zone, so a call made at 23:30 in New
/// York counts toward that New York day although it is already the next day in UTC. A call whose
/// cost was not recorded counts as $0.
///
/// Example: scry calls at 04:30 UTC ($1.00) and 05:30 UTC ($0.25) on 17 November. Counted from
/// 17 November in New York (UTC-5), scry has spent $0.25: the first call was still 16 November
/// there.
pub fn spent_since(
    errands: &[Errand],
    product: &str,
    since: NaiveDate,
    offset: FixedOffset,
) -> f64 {
    errands
        .iter()
        .filter(|e| e.product == product && e.at.with_timezone(&offset).date_naive() >= since)
        .map(|e| e.cost_usd.unwrap_or(0.0))
        .sum()
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;

    fn errand(at: &str, product: &str, cost: Option<f64>) -> Errand {
        Errand {
            at: at.parse().unwrap(),
            product: product.into(),
            verb: "find".into(),
            model: "sonnet".into(),
            provider: "claude".into(),
            cost_usd: cost,
            input_tokens: 0,
            output_tokens: 0,
            seconds: 0.0,
            ok: true,
            note: None,
        }
    }

    #[test]
    fn spent_counts_one_product_from_a_local_day() {
        let es = vec![
            errand("2026-11-17T04:30:00Z", "scry", Some(1.0)),
            errand("2026-11-17T05:30:00Z", "scry", Some(0.25)),
            errand("2026-11-17T06:00:00Z", "conjure", Some(5.0)),
        ];
        let est = FixedOffset::west_opt(5 * 3600).unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap();
        assert_eq!(spent_since(&es, "scry", day, est), 0.25);
    }

    #[test]
    fn an_errand_with_no_cost_counts_zero() {
        let es = vec![errand("2026-11-17T12:00:00Z", "scry", None)];
        let day = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap();
        assert_eq!(
            spent_since(&es, "scry", day, FixedOffset::east_opt(0).unwrap()),
            0.0
        );
    }
}
