//! `scry cost`.

use std::io::Write;

use anyhow::Result;
use chrono::{FixedOffset, NaiveDate};
use serde_json::json;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;

use super::format::table;
use crate::adapters::json_finding_store::JsonFindingStore;
use crate::domain::log_text::count;
use crate::usecases::report_cost::report_cost;

/// What scry spent, a local day (in OFFSET) a row, from SINCE on; `decide`
/// errands count into the total only.
pub(super) fn cost(
    project: &Project,
    since: Option<&str>,
    offset: FixedOffset,
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let since = since
        .map(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("--since {s}: give a day as YYYY-MM-DD"))
        })
        .transpose()?;
    let r = report_cost(
        &JsonlLedger::for_project(project)?,
        &JsonFindingStore::for_project(project)?,
        offset,
        since,
    )?;
    // a store that does not read is said, not counted as nothing waiting
    if let Some(e) = &r.store_error {
        writeln!(err, "scry: {e}")?;
    }
    let waiting = r.waiting_units;
    if json {
        let v: Vec<_> = r
            .days
            .iter()
            .map(|d| {
                json!({
                    "day": d.day.to_string(),
                    "reviews": d.reviews,
                    "checks": d.checks,
                    "review_cost": d.review_cost,
                    "check_cost": d.check_cost,
                    "total_cost": d.total_cost,
                })
            })
            .collect();
        writeln!(out, "{}", json!({ "days": v, "waiting_units": waiting }))?;
    } else {
        let t: Vec<Vec<String>> = r
            .days
            .iter()
            .map(|d| {
                vec![
                    d.day.to_string(),
                    d.reviews.to_string(),
                    d.checks.to_string(),
                    format!("{:.2}", d.review_cost),
                    format!("{:.2}", d.check_cost),
                    format!("{:.2}", d.total_cost),
                ]
            })
            .collect();
        write!(
            out,
            "{}",
            table(
                &["day", "reviews", "checks", "review $", "check $", "total $"],
                &t
            )
        )?;
        if let Some(n) = waiting {
            writeln!(out, "waiting on the cap: {}", count(n, "unit"))?;
        }
    }
    Ok(0)
}
