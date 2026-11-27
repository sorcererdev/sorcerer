//! `lackey cost`: what lackey spent, a local day a row, with each verb's calls.

use std::io::Write;

use anyhow::Result;
use chrono::{FixedOffset, NaiveDate};
use serde_json::json;
use sorcerer_core::ports::ledger::Ledger;

use super::format::table;
use crate::domain::log_text::usd;
use crate::usecases::report_cost::{DayCost, report_cost};

/// The verbs, in the order the table shows them: Jev's triage, answers, claim checks, tasks.
const VERBS: [&str; 4] = ["decide", "answer", "check", "task"];

pub(super) fn cost(
    ledger: &dyn Ledger,
    since: Option<&str>,
    offset: FixedOffset,
    json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let since = since.map(day_of).transpose()?;
    let days = report_cost(ledger, offset, since)?;
    if json {
        write!(out, "{}", cost_json_text(&days))?;
    } else {
        write!(out, "{}", cost_table_text(&days))?;
    }
    Ok(0)
}

fn day_of(since: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(since, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("--since {since}: give a day as YYYY-MM-DD"))
}

// The command's text.

fn cost_json_text(days: &[DayCost]) -> String {
    let days: Vec<_> = days
        .iter()
        .map(|day| {
            let verbs: serde_json::Map<String, serde_json::Value> = day
                .verbs
                .iter()
                .map(|(verb, (calls, cost))| {
                    (verb.clone(), json!({ "calls": calls, "cost": cost }))
                })
                .collect();
            json!({ "day": day.day.to_string(), "verbs": verbs, "cost": day.total_cost })
        })
        .collect();
    format!("{}\n", json!({ "days": days }))
}

/// `day  triage  answers  checks  tasks  cost`, each verb's calls.
fn cost_table_text(days: &[DayCost]) -> String {
    let rows: Vec<Vec<String>> = days
        .iter()
        .map(|day| {
            let calls = VERBS.iter().map(|verb| {
                day.verbs
                    .get(*verb)
                    .map_or(0, |(calls, _)| *calls)
                    .to_string()
            });
            std::iter::once(day.day.to_string())
                .chain(calls)
                .chain(std::iter::once(usd(day.total_cost)))
                .collect()
        })
        .collect();
    table(
        &["day", "triage", "answers", "checks", "tasks", "cost"],
        &rows,
    )
}
