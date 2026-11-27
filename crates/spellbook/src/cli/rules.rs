//! `spellbook rules FILE [--json]`: the rules that look at a file, so a writer (a person or a
//! model) can keep them before the file is checked.

use std::io::Write;
use std::path::Path;

use anyhow::{Result, anyhow};
use serde::Serialize;

use crate::adapters::wiring::Wired;
use crate::domain::rule::Rule;

/// One rule as `rules` prints it.
#[derive(Serialize)]
struct RuleJson<'r> {
    id: &'r str,
    severity: &'static str,
    message: &'r str,
    #[serde(skip_serializing_if = "Option::is_none")]
    why: Option<&'r str>,
}

/// Prints a row per rule that applies to the file (`id  severity  message`, then its why),
/// or `no rules look at FILE`. Exit 0.
///
/// Example: for `src/bistro/booking/commands/seat.rs`, `commands-use-ports  error  Commands talk
/// to ports`.
pub(super) fn rules(wired: &Wired, file: &Path, as_json: bool, out: &mut dyn Write) -> Result<i32> {
    let checking = wired.checking().map_err(|why| anyhow!(why))?;
    let name = file.to_string_lossy().replace('\\', "/");
    let here: Vec<&Rule> = checking
        .rules
        .iter()
        .filter(|rule| rule.applies_to(&name))
        .collect();
    if as_json {
        let rows: Vec<RuleJson> = here
            .iter()
            .map(|rule| RuleJson {
                id: &rule.id,
                severity: rule.severity.word(),
                message: &rule.message,
                why: rule.why.as_deref(),
            })
            .collect();
        writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "rules": rows }))?
        )?;
        return Ok(0);
    }
    if here.is_empty() {
        writeln!(out, "no rules look at {name}")?;
    }
    for rule in here {
        writeln!(
            out,
            "{}  {}  {}",
            rule.id,
            rule.severity.word(),
            rule.message
        )?;
        if let Some(why) = &rule.why {
            writeln!(out, "    {}", why.trim())?;
        }
    }
    Ok(0)
}
