//! `spellbook new <generator> [--set input=value]… [--plan] [--json]`: plans a generation,
//! shows it, and (without `--plan`) writes it.

use std::io::Write;

use anyhow::Result;
use serde_json::{Value, json};

use crate::adapters::wiring::Wired;
use crate::domain::holes::Hole;
use crate::domain::inputs::Given;
use crate::domain::plan::{FileStatus, Plan, PlanStop, plan_stop_text};
use crate::ports::generators::Generators;
use crate::ports::workspace::write_refused_text;
use crate::usecases::apply_plan::{AppliedGeneration, ChangesLanded, apply_plan};
use crate::usecases::plan_generation::plan_generation;

use super::format::{columns, count};

/// What the person asked `new` for.
pub(super) struct NewRequest {
    pub(super) generator: String,
    pub(super) set: Vec<String>,
    pub(super) only_plan: bool,
    pub(super) json: bool,
}

/// Plans the generation, then prints it and writes it (or, with `--plan`, prints it with each
/// file's text and writes nothing). A stop (a missing input, a path refused, a file there
/// already) prints its sentence and exits 2, and nothing is written.
pub(super) fn new(
    wired: &Wired,
    request: &NewRequest,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let plan = match planned(wired, request) {
        Ok(plan) => plan,
        Err(stop) => return print_stop(&stop, request, out, err),
    };
    if request.only_plan {
        print_planned(&plan, request.json, out)?;
        return Ok(0);
    }
    let applied = apply_plan(&wired.apply_deps(), plan, ChangesLanded::NotYet)?;
    print_applied(&applied, request.json, out, err)
}

fn planned(wired: &Wired, request: &NewRequest) -> Result<Plan, PlanStop> {
    let given = Given::from_pairs(&request.set)
        .map_err(|pair| PlanStop::Template(format!("--set {pair}: write it as INPUT=VALUE")))?;
    let generator = match wired.generators.named(&request.generator) {
        Some(Ok(generator)) => generator,
        Some(Err(problem)) => return Err(PlanStop::Generator(problem.to_string())),
        None => {
            return Err(PlanStop::Generator(format!(
                "no generator named {} (spellbook list shows them)",
                request.generator
            )));
        }
    };
    plan_generation(&wired.plan_deps(), &generator, &given)
}

fn print_stop(
    stop: &PlanStop,
    request: &NewRequest,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    writeln!(err, "spellbook: {}", plan_stop_text(stop))?;
    if let PlanStop::Input(input_stop) = stop
        && let Some(input) = input_stop.input()
    {
        writeln!(err, "  give it with --set {input}=VALUE")?;
    }
    if request.json {
        writeln!(out, "{}", stop_json(stop))?;
    }
    Ok(2)
}

fn stop_json(stop: &PlanStop) -> Value {
    match stop {
        PlanStop::Input(input_stop) => json!({
            "stop": plan_stop_text(stop),
            "input": input_stop.input(),
            "choices": input_stop.choices(),
        }),
        _ => json!({ "stop": plan_stop_text(stop) }),
    }
}

fn print_planned(plan: &Plan, as_json: bool, out: &mut dyn Write) -> Result<()> {
    if as_json {
        let report = report_json(plan, false, &[], &skipped_paths(plan));
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
        return Ok(());
    }
    write!(out, "{}", summary_text(plan, &plan_paths(plan)))?;
    for file in plan.new_files() {
        write!(out, "\n--- {}\n{}", file.path, file.text)?;
        if !file.text.ends_with('\n') && !file.text.is_empty() {
            writeln!(out)?;
        }
    }
    writeln!(out, "\n(a plan: nothing was written)")?;
    Ok(())
}

fn print_applied(
    applied: &AppliedGeneration,
    as_json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    if as_json {
        let report = report_json(&applied.plan, true, &applied.written, &applied.skipped);
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        write!(out, "{}", summary_text(&applied.plan, &applied.written))?;
    }
    match &applied.refused {
        None => Ok(0),
        Some((path, refused)) => {
            writeln!(
                err,
                "{}",
                refused_text(path, &write_refused_text(refused), &applied.written)
            )?;
            Ok(2)
        }
    }
}

fn refused_text(path: &str, why: &str, written: &[String]) -> String {
    let before = if written.is_empty() {
        "nothing was written".to_string()
    } else {
        format!("written before it: {}", written.join(", "))
    };
    format!(
        "spellbook: {path} was not written: {why}; {before}; the files after it were not written"
    )
}

/// The new files' paths: in a plan, the files that would be written.
fn plan_paths(plan: &Plan) -> Vec<String> {
    plan.new_files()
        .iter()
        .map(|file| file.path.clone())
        .collect()
}

fn skipped_paths(plan: &Plan) -> Vec<String> {
    plan.files
        .iter()
        .filter(|file| file.status == FileStatus::Skipped)
        .map(|file| file.path.clone())
        .collect()
}

/// The generation in a few lines: the files (`+` written, `=` left as they were), the holes to
/// fill at their first place, and what the inputs filled.
///
/// Example:
/// ```text
/// command (2 files, 3 holes)
///   + src/bistro/booking/commands/cancel_table.py  (30 lines)
///   = src/bistro/booking/commands/__init__.py  (exists, skipped)
/// holes:
///   handle  src/bistro/booking/commands/cancel_table.py:30  block  its first case
/// (filled from the inputs: purpose "Cancel table.")
/// ```
fn summary_text(plan: &Plan, written: &[String]) -> String {
    let holes = plan.placeholder_holes();
    let mut text = format!(
        "{} ({}, {})\n",
        plan.generator,
        count(written.len(), "file", "files"),
        count(holes.len(), "hole", "holes")
    );
    let file_rows: Vec<Vec<String>> = plan
        .files
        .iter()
        .filter(|file| file.status != FileStatus::New || written.contains(&file.path))
        .map(|file| match file.status {
            FileStatus::New => vec![
                format!("+ {}", file.path),
                format!("({} lines)", file.lines),
            ],
            FileStatus::Changed => vec![
                format!("~ {}", file.path),
                format!("(+{} \u{2212}{})", file.added, file.removed),
            ],
            FileStatus::Skipped => vec![format!("= {}", file.path), "(exists, skipped)".into()],
            FileStatus::Unchanged => vec![format!("= {}", file.path), "(unchanged)".into()],
        })
        .collect();
    text.push_str(&columns("  ", &file_rows));
    if !holes.is_empty() {
        text.push_str("holes:\n");
        text.push_str(&columns(
            "  ",
            &holes.iter().map(|hole| hole_row(hole)).collect::<Vec<_>>(),
        ));
    }
    text.push_str(&derived_text(&plan.derived_holes()));
    text
}

fn hole_row(hole: &Hole) -> Vec<String> {
    let place = &hole.places[0];
    vec![
        hole.name.clone(),
        format!("{}:{}", place.file, place.line),
        hole.kind.clone(),
        hole.instruction.clone(),
    ]
}

fn derived_text(derived: &[&Hole]) -> String {
    if derived.is_empty() {
        return String::new();
    }
    let filled: Vec<String> = derived
        .iter()
        .map(|hole| format!("{} \"{}\"", hole.name, hole.places[0].text))
        .collect();
    format!("(filled from the inputs: {})\n", filled.join(", "))
}

/// What an agent reads: the plan, whether it was applied, the files written and skipped, and
/// the holes to fill.
fn report_json(plan: &Plan, applied: bool, written: &[String], skipped: &[String]) -> Value {
    json!({
        "plan": plan,
        "applied": applied,
        "written": written,
        "skipped": skipped,
        "holes": plan.placeholder_holes(),
    })
}
