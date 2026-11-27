//! `spellbook sync [--init NAME] [--check] [--plan] [--json]`: refreshes the owned regions,
//! sets one up, checks them, or shows the refresh.

use std::io::Write;

use anyhow::Result;
use serde_json::json;

use crate::adapters::wiring::Wired;
use crate::domain::plan::{Plan, PlanStop, plan_stop_text};
use crate::ports::workspace::write_refused_text;
use crate::usecases::apply_plan::{AppliedGeneration, ChangesLanded, apply_plan};
use crate::usecases::sync_regions::{NO_REGIONS, SyncMode, SyncPlan, plan_sync};

use super::format::{columns, count, file_row, hunks_text, notes_text};

/// What the person asked `sync` for.
pub(super) struct SyncRequest {
    /// The region to set up (`--init NAME`).
    pub(super) init: Option<String>,
    pub(super) check: bool,
    pub(super) only_plan: bool,
    pub(super) json: bool,
}

/// Plans the sync, then (1) under `--check` prints what is out of date or not set up and exits
/// 1 when anything is; (2) says the regions are up to date when there is nothing to write;
/// (3) under `--plan` prints the rows and each file's hunks; (4) else writes it. The exit code
/// is 0 when the regions are up to date or were brought up to date, 1 when the plan stopped
/// (broken markers, a refused write) or `--check` found work, 2 when it could not run (a
/// `regions.toml` that does not load, no region by that name).
pub(super) fn sync(
    wired: &Wired,
    request: &SyncRequest,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let mode = match &request.init {
        Some(name) => SyncMode::Init(name.clone()),
        None => SyncMode::Refresh,
    };
    let planned = match plan_sync(&wired.plan_deps(), &mode) {
        Ok(planned) => planned,
        Err(stop) => return print_stop(&stop, request.json, out, err),
    };
    if request.check {
        return print_check(&planned, request.json, out);
    }
    if planned.plan.files.is_empty() {
        print_up_to_date(&planned, request.json, out)?;
        return Ok(0);
    }
    if request.only_plan {
        print_planned(&planned.plan, request.json, out)?;
        return Ok(0);
    }
    let applied = apply_plan(&wired.apply_deps(), planned.plan, ChangesLanded::NotYet)?;
    print_applied(&applied, request.json, out, err)
}

fn print_stop(
    stop: &PlanStop,
    as_json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    writeln!(err, "spellbook: {}", plan_stop_text(stop))?;
    if as_json {
        writeln!(out, "{}", json!({ "stop": plan_stop_text(stop) }))?;
    }
    Ok(match stop {
        PlanStop::RegionsDoNotLoad(_) | PlanStop::RegionNotThere { .. } => 2,
        _ => 1,
    })
}

/// `--check`: a line for each block out of date and each region not set up, then `nothing
/// written (--check)`; or that the regions are up to date.
fn print_check(planned: &SyncPlan, as_json: bool, out: &mut dyn Write) -> Result<i32> {
    let has_work = !planned.plan.files.is_empty() || !planned.plan.notes.is_empty();
    if as_json {
        writeln!(out, "{}", serde_json::to_string_pretty(&planned.plan)?)?;
        return Ok(i32::from(has_work));
    }
    if !has_work {
        write!(out, "{}", up_to_date_text(planned))?;
        return Ok(0);
    }
    for file in &planned.plan.files {
        writeln!(
            out,
            "region {} is out of date in {} (+{} \u{2212}{})  (run spellbook sync)",
            file.region.as_deref().unwrap_or_default(),
            file.path,
            file.added,
            file.removed
        )?;
    }
    write!(out, "{}", notes_text(&planned.plan))?;
    writeln!(out, "nothing written (--check)")?;
    Ok(1)
}

fn print_up_to_date(planned: &SyncPlan, as_json: bool, out: &mut dyn Write) -> Result<()> {
    if as_json {
        let report = json!({ "plan": planned.plan, "applied": false, "changed": [] });
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
        return Ok(());
    }
    write!(out, "{}", up_to_date_text(planned))?;
    write!(out, "{}", notes_text(&planned.plan))?;
    Ok(())
}

/// `regions up to date (5 regions)`, or that the project declares none.
fn up_to_date_text(planned: &SyncPlan) -> String {
    if planned.declares_no_regions() {
        return format!("{NO_REGIONS}\n");
    }
    format!(
        "regions up to date ({})\n",
        count(planned.blocks, "region", "regions")
    )
}

fn print_planned(plan: &Plan, as_json: bool, out: &mut dyn Write) -> Result<()> {
    if as_json {
        writeln!(out, "{}", serde_json::to_string_pretty(plan)?)?;
        return Ok(());
    }
    write!(out, "{}", summary_text(plan))?;
    for file in &plan.files {
        write!(out, "{}", hunks_text(file))?;
    }
    writeln!(out, "nothing written (--plan)")?;
    Ok(())
}

fn print_applied(
    applied: &AppliedGeneration,
    as_json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    if as_json {
        let report = json!({
            "plan": applied.plan,
            "applied": applied.refused.is_none(),
            "changed": applied.changed,
        });
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        write!(out, "{}", summary_text(&applied.plan))?;
    }
    match &applied.refused {
        None => Ok(0),
        Some((path, refused)) => {
            writeln!(
                err,
                "spellbook: {path} was not written: {}",
                write_refused_text(refused)
            )?;
            Ok(1)
        }
    }
}

/// The sync in a few lines: how many files it changes, a row for each, and the notes.
///
/// Example:
/// ```text
/// sync (1 file changed)
///   ~ src/booking/queries/mod.rs  (region modules: +0 −1)
/// ```
fn summary_text(plan: &Plan) -> String {
    let rows: Vec<Vec<String>> = plan.files.iter().map(file_row).collect();
    format!(
        "sync ({} changed)\n{}{}",
        count(plan.files.len(), "file", "files"),
        columns("  ", &rows),
        notes_text(plan)
    )
}
