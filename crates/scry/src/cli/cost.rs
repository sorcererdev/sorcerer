//! `scry cost`: what scry spent, a day a row, and how many units wait for
//! the next run because a cap stopped them.

use anyhow::Result;
use chrono::{FixedOffset, NaiveDate};
use serde_json::json;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;

use super::Console;
use super::format::{Output, table};
use crate::adapters::json_finding_store::JsonFindingStore;
use crate::domain::log_text::count;
use crate::usecases::report_cost::{CostReport, report_cost};

/// Prints what scry spent in each of the user's local days (`offset` is
/// their time zone), from the `--since` day on when given. Review calls and
/// claim checks have their own columns; Jev's questions count only in the
/// total.
pub(super) fn cost(
    project: &Project,
    since: Option<&str>,
    offset: FixedOffset,
    output: Output,
    console: &mut Console,
) -> Result<i32> {
    let since = since.map(day_of).transpose()?;
    let report = report_cost(
        &JsonlLedger::for_project(project)?,
        &JsonFindingStore::for_project(project)?,
        offset,
        since,
    )?;
    // a store that does not read is said, not counted as nothing waiting
    if let Some(e) = &report.store_error {
        writeln!(console.err, "scry: {e}")?;
    }
    let text = match output {
        Output::Json => cost_json_text(&report),
        Output::Text => cost_table_text(&report),
    };
    write!(console.out, "{text}")?;
    Ok(0)
}

/// The `--since` day, written `2026-11-20`.
fn day_of(since: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(since, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("--since {since}: give a day as YYYY-MM-DD"))
}

// The command's text.

/// One JSON object: the days, and the units waiting on the cap.
fn cost_json_text(report: &CostReport) -> String {
    let days: Vec<_> = report
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
        json!({ "days": days, "waiting_units": report.waiting_units })
    )
}

/// A table with a row for each day, then how many units wait on the cap
/// (left out when none wait, or the store could not be read).
fn cost_table_text(report: &CostReport) -> String {
    let rows: Vec<Vec<String>> = report
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
    let days = table(
        &["day", "reviews", "checks", "review $", "check $", "total $"],
        &rows,
    );
    let waiting = report
        .waiting_units
        .filter(|&n| n > 0)
        .map(|n| format!("waiting on the cap: {}\n", count(n, "unit")))
        .unwrap_or_default();
    format!("{days}{waiting}")
}
