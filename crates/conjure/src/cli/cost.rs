//! `conjure cost`: what conjure spent, a local day a row.

use std::io::Write;

use anyhow::Result;
use chrono::{FixedOffset, NaiveDate};
use serde_json::json;
use sorcerer_core::ports::ledger::Ledger;

use super::format::table;
use crate::domain::log_text::usd;
use crate::usecases::report_cost::{CostReport, report_cost};

/// Prints what conjure spent in each of the user's local days (`offset` is their time zone),
/// from the `--since` day on when given, as a table or as JSON.
pub(super) fn cost(
    ledger: &dyn Ledger,
    since: Option<&str>,
    offset: FixedOffset,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let since = since.map(day_of).transpose()?;
    let report = report_cost(ledger, offset, since)?;
    if json {
        write!(out, "{}", cost_json_text(&report))?;
    } else {
        write!(out, "{}", cost_table_text(&report))?;
    }
    Ok(0)
}

/// The `--since` day, written `2026-11-20`.
fn day_of(since: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(since, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("--since {since}: give a day as YYYY-MM-DD"))
}

// The command's text.

fn cost_json_text(report: &CostReport) -> String {
    let days: Vec<_> = report
        .days
        .iter()
        .map(|d| {
            json!({
                "day": d.day.to_string(),
                "changes": d.changes,
                "try_harder": d.try_harders,
                "retries": d.retries,
                "cost": d.total_cost,
            })
        })
        .collect();
    format!("{}\n", json!({ "days": days }))
}

fn cost_table_text(report: &CostReport) -> String {
    let rows: Vec<Vec<String>> = report
        .days
        .iter()
        .map(|d| {
            vec![
                d.day.to_string(),
                d.changes.to_string(),
                d.try_harders.to_string(),
                d.retries.to_string(),
                usd(d.total_cost),
            ]
        })
        .collect();
    table(&["day", "changes", "try harder", "retries", "cost"], &rows)
}
