//! `spellbook list`: the generators, a row each.

use std::io::Write;

use anyhow::Result;

use crate::adapters::generator_dir::GENERATORS_DIR;
use crate::adapters::wiring::Wired;
use crate::usecases::list_generators::{GeneratorSummary, list_generators};

use super::format::columns;

/// Prints a row per generator: its name, what it makes, and its inputs; a generator that does
/// not load is a row starting with `!` and its problem.
pub(super) fn list(wired: &Wired, out: &mut dyn Write) -> Result<i32> {
    let generators = list_generators(&wired.generators);
    if generators.is_empty() {
        writeln!(out, "{}", no_generators_text())?;
        return Ok(0);
    }
    let rows: Vec<Vec<String>> = generators
        .iter()
        .map(|loaded| match loaded {
            Ok(summary) => generator_row(summary),
            Err(problem) => vec![format!("! {}", problem.generator), problem.why.clone()],
        })
        .collect();
    write!(out, "{}", columns("", &rows))?;
    Ok(0)
}

fn generator_row(summary: &GeneratorSummary) -> Vec<String> {
    let inputs: Vec<&str> = summary
        .inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect();
    vec![
        summary.id.clone(),
        summary.summary.clone(),
        format!("inputs: {}", inputs.join(", ")),
    ]
}

pub(super) fn no_generators_text() -> String {
    format!("no generators: add one as {GENERATORS_DIR}/<name>/generator.toml")
}
