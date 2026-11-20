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
use crate::usecases::report_cost::{CostReport, report_cost};

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
    let since = since.map(day_of).transpose()?;
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
    if json {
        write!(out, "{}", cost_json_text(&r))?;
    } else {
        write!(out, "{}", cost_table_text(&r))?;
    }
    Ok(0)
}

/// `--since`'s day.
fn day_of(since: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(since, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("--since {since}: give a day as YYYY-MM-DD"))
}

// The command's text.

/// One JSON object: the days and the units waiting on the cap.
fn cost_json_text(r: &CostReport) -> String {
    let days: Vec<_> = r
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
    format!(
        "{}\n",
        json!({ "days": days, "waiting_units": r.waiting_units })
    )
}

/// A row a day, then the units waiting on the cap (when the store read).
fn cost_table_text(r: &CostReport) -> String {
    let rows: Vec<Vec<String>> = r
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
    let mut s = table(
        &["day", "reviews", "checks", "review $", "check $", "total $"],
        &rows,
    );
    if let Some(n) = r.waiting_units {
        s.push_str(&format!("waiting on the cap: {}\n", count(n, "unit")));
    }
    s
}
