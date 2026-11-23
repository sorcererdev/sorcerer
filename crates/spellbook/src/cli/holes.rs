//! `spellbook holes [FILE…] [--json]`: the holes still open.

use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;

use crate::adapters::wiring::Wired;
use crate::domain::record::OpenHole;
use crate::usecases::open_holes::open_holes;

use super::format::{columns, count};

/// Prints a row per open hole place (`name  file:line  kind  instruction`), then how many.
pub(super) fn holes(
    wired: &Wired,
    files: &[PathBuf],
    as_json: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let only = (!files.is_empty()).then_some(files);
    let open = open_holes(&wired.holes_deps(), only)?;
    if as_json {
        writeln!(out, "{}", serde_json::to_string_pretty(&open)?)?;
        return Ok(0);
    }
    write!(
        out,
        "{}",
        columns("", &open.iter().map(hole_row).collect::<Vec<_>>())
    )?;
    writeln!(out, "{}", count(open.len(), "open hole", "open holes"))?;
    Ok(0)
}

fn hole_row(hole: &OpenHole) -> Vec<String> {
    vec![
        hole.name.clone(),
        format!("{}:{}", hole.file, hole.line),
        hole.kind.clone(),
        hole.instruction.clone(),
    ]
}
