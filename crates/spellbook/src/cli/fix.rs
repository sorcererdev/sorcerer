//! `spellbook fix FILE:LINE [--rule ID] [--fix N|LABEL] [--set k=v]… [--plan] [--json]`: plans
//! the fix of the finding on that line, shows it, and (without `--plan`) applies it.

use std::io::Write;

use anyhow::{Result, anyhow};
use serde_json::json;

use crate::adapters::wiring::Wired;
use crate::domain::inputs::Given;
use crate::domain::plan::{FileStatus, Plan, PlanStop, PlannedFile, plan_stop_text};
use crate::domain::rule::Rule;
use crate::ports::generators::Generators;
use crate::ports::workspace::write_refused_text;
use crate::usecases::apply_plan::{AppliedGeneration, ChangesLanded, apply_plan};
use crate::usecases::plan_fix::{FindingAt, plan_fix};

use super::format::{columns, count};
use super::new::hole_row;

/// What the person asked `fix` for: the finding's file (relative to the root, with `/`) and line,
/// the rule and the fix when they pick one.
pub(super) struct FixRequest {
    pub(super) file: String,
    pub(super) line: u32,
    pub(super) rule: Option<String>,
    pub(super) fix: Option<String>,
    pub(super) set: Vec<String>,
    pub(super) only_plan: bool,
    pub(super) json: bool,
}

/// Plans the fix, prints it, and (without `--plan`) applies it. Exit 0 when applied (or
/// planned), 1 when the plan stopped or a file was refused, 2 when an input is missing (its
/// choices are printed, as `new` does), 3 when there is nothing to do (the fix is applied).
pub(super) fn fix(
    wired: &Wired,
    request: &FixRequest,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let checking = wired.checking().map_err(|why| anyhow!(why))?;
    let planned = Given::from_pairs(&request.set)
        .map_err(|pair| PlanStop::Template(format!("--set {pair}: write it as INPUT=VALUE")))
        .and_then(|given| {
            let index = fix_index(wired, &checking.rules, request)?;
            let at = FindingAt {
                file: request.file.clone(),
                line: request.line,
                rule: request.rule.clone(),
                index,
            };
            plan_fix(&checking.fix_deps(), &at, &given)
        });
    let plan = match planned {
        Ok(plan) => plan,
        Err(stop) => return print_stop(&stop, request.json, out, err),
    };
    if request.only_plan {
        print_planned(&plan, request.json, out)?;
        return Ok(0);
    }
    let applied = apply_plan(&wired.apply_deps(), plan, ChangesLanded::NotYet)?;
    print_applied(&applied, request.json, out, err)
}

/// `--fix`: a number from 1, or a fix's label (or its generator's name); the first fix when
/// none is given.
fn fix_index(wired: &Wired, rules: &[Rule], request: &FixRequest) -> Result<usize, PlanStop> {
    let Some(wanted) = &request.fix else {
        return Ok(1);
    };
    if let Ok(number) = wanted.parse::<usize>() {
        return Ok(number);
    }
    let rules_here = rules
        .iter()
        .filter(|rule| rule.applies_to(&request.file))
        .filter(|rule| request.rule.as_ref().is_none_or(|id| *id == rule.id));
    for rule in rules_here {
        for (at, fix) in rule.fixes.iter().enumerate() {
            let name = match wired.generators.named(&fix.generator) {
                Some(Ok(generator)) => generator.name,
                _ => String::new(),
            };
            if fix.label.as_deref() == Some(wanted.as_str()) || name == *wanted {
                return Ok(at + 1);
            }
        }
    }
    Err(PlanStop::Generator(format!(
        "--fix {wanted}: no fix by that label on {}:{}",
        request.file, request.line
    )))
}

fn print_stop(
    stop: &PlanStop,
    as_json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    if as_json {
        writeln!(out, "{}", json!({ "stop": plan_stop_text(stop) }))?;
    }
    if let PlanStop::NothingToDo(_) = stop {
        if !as_json {
            writeln!(out, "{}", plan_stop_text(stop))?;
        }
        return Ok(3);
    }
    writeln!(err, "spellbook: {}", plan_stop_text(stop))?;
    match stop {
        PlanStop::Input(input_stop) => {
            if let Some(input) = input_stop.input() {
                writeln!(err, "  give it with --set {input}=VALUE")?;
            }
            Ok(2)
        }
        _ => Ok(1),
    }
}

fn print_planned(plan: &Plan, as_json: bool, out: &mut dyn Write) -> Result<()> {
    if as_json {
        writeln!(out, "{}", serde_json::to_string_pretty(plan)?)?;
        return Ok(());
    }
    write!(out, "{}", summary_text(plan))?;
    for file in ordered_files(plan) {
        match file.status {
            FileStatus::Changed => write!(out, "{}", hunks_text(file))?,
            FileStatus::New => {
                write!(out, "--- {}\n{}", file.path, file.text)?;
                if !file.text.ends_with('\n') {
                    writeln!(out)?;
                }
            }
            FileStatus::Skipped | FileStatus::Unchanged => {}
        }
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
            "written": applied.written,
        });
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        write!(out, "{}", summary_text(&applied.plan))?;
    }
    match &applied.refused {
        None => {
            if !as_json {
                writeln!(out, "{}", applied_text(applied))?;
            }
            Ok(0)
        }
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

/// The fix in a few lines: its label and generator with what it changes, a row for each file
/// (`+` new, `~` changed, `=` left as it is), the holes to fill.
///
/// Example:
/// ```text
/// Use a port the service has: use-port (2 files changed, 0 new, 0 holes)
///   ~ src/bistro/booking/commands/close_table.py        (+3 −1)
///   ~ src/bistro/booking/commands/close_table__test.py  (+2 −1)
/// ```
fn summary_text(plan: &Plan) -> String {
    let holes = plan.placeholder_holes();
    let label = plan
        .fix
        .as_ref()
        .map_or(plan.generator.as_str(), |fix| fix.label.as_str());
    let mut text = format!(
        "{label}: {} ({} changed, {} new, {})\n",
        plan.generator,
        count(plan.changed_files().len(), "file", "files"),
        plan.new_files().len(),
        count(holes.len(), "hole", "holes")
    );
    let rows: Vec<Vec<String>> = ordered_files(plan).into_iter().map(file_row).collect();
    text.push_str(&columns("  ", &rows));
    if !holes.is_empty() {
        text.push_str("holes:\n");
        let hole_rows: Vec<Vec<String>> = holes.iter().map(|hole| hole_row(hole)).collect();
        text.push_str(&columns("  ", &hole_rows));
    }
    text
}

/// New files first, then changed ones, then the ones left as they are.
fn ordered_files(plan: &Plan) -> Vec<&PlannedFile> {
    let rank = |file: &&PlannedFile| match file.status {
        FileStatus::New => 0,
        FileStatus::Changed => 1,
        FileStatus::Skipped | FileStatus::Unchanged => 2,
    };
    let mut files: Vec<&PlannedFile> = plan.files.iter().collect();
    files.sort_by_key(rank);
    files
}

fn file_row(file: &PlannedFile) -> Vec<String> {
    match file.status {
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
    }
}

/// A changed file's diff: its path, then each hunk's header and lines.
fn hunks_text(file: &PlannedFile) -> String {
    let mut text = format!("--- {}\n", file.path);
    for hunk in &file.hunks {
        text.push_str(&hunk.header);
        text.push('\n');
        for line in &hunk.lines {
            text.push_str(line);
            text.push('\n');
        }
    }
    text
}

/// The end line of an applied fix.
fn applied_text(applied: &AppliedGeneration) -> String {
    format!(
        "applied: {} changed, {} written",
        count(applied.changed.len(), "file", "files"),
        applied.written.len()
    )
}
