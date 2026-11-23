//! `spellbook show <generator>`: everything about one generator, with its inputs' choices now.

use std::io::Write;

use anyhow::Result;
use sorcerer_core::domain::templates::{Piece, pieces};

use crate::adapters::wiring::Wired;
use crate::domain::generator::{EditBody, Generator, InputKind, OnExists};
use crate::domain::inputs::{Given, input_stop_text};
use crate::ports::generators::Generators;
use crate::usecases::choices_for::choices_for;

use super::format::columns;

/// Prints the generator's description, its inputs (with the choices the project offers now),
/// the files it creates, its holes and its examples.
pub(super) fn show(
    wired: &Wired,
    id: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let generator = match wired.generators.named(id) {
        Some(Ok(generator)) => generator,
        Some(Err(problem)) => {
            writeln!(err, "spellbook: {problem}")?;
            return Ok(2);
        }
        None => {
            writeln!(
                err,
                "spellbook: no generator named {id} (spellbook list shows them)"
            )?;
            return Ok(2);
        }
    };
    write!(out, "{}", header_text(&generator))?;
    write!(
        out,
        "inputs:\n{}",
        columns("  ", &input_rows(wired, &generator))
    )?;
    write!(out, "creates:\n{}", columns("  ", &edit_rows(&generator)))?;
    write!(out, "holes:\n{}", columns("  ", &hole_rows(&generator)))?;
    if !generator.examples.is_empty() {
        writeln!(out, "examples:")?;
        for example in &generator.examples {
            writeln!(out, "  {example}")?;
        }
    }
    Ok(0)
}

fn header_text(generator: &Generator) -> String {
    let mut text = format!(
        "{} ({}): {}\n",
        generator.id, generator.language, generator.name
    );
    for line in [&generator.summary, &generator.description] {
        if !line.is_empty() {
            text.push_str(&format!("  {line}\n"));
        }
    }
    text
}

fn input_rows(wired: &Wired, generator: &Generator) -> Vec<Vec<String>> {
    generator
        .inputs
        .iter()
        .map(|(name, input)| {
            let now = match &input.kind {
                InputKind::Enum { .. } => {
                    match choices_for(&wired.view, generator, &Given::default(), name) {
                        Ok(choices) => format!("choices now: {}", choices.join(", ")),
                        Err(stop) => format!("({})", input_stop_text(&stop)),
                    }
                }
                _ => String::new(),
            };
            let kind = if input.optional {
                format!("{} (optional)", input.kind.word())
            } else {
                input.kind.word().to_string()
            };
            vec![name.clone(), kind, input.prompt.clone(), now]
        })
        .collect()
}

fn edit_rows(generator: &Generator) -> Vec<Vec<String>> {
    generator
        .edits
        .iter()
        .map(|edit| {
            let body = match &edit.body {
                EditBody::Template(name) => format!("from templates/{name}"),
                EditBody::Text(_) => "a fixed text".into(),
            };
            let exists = match edit.on_exists {
                OnExists::Error => "stops if it exists",
                OnExists::Skip => "skipped if it exists",
            };
            vec![edit.create.clone(), body, exists.into()]
        })
        .collect()
}

/// Each hole once, in the order the templates hold them, marked when the inputs fill it.
fn hole_rows(generator: &Generator) -> Vec<Vec<String>> {
    let mut seen: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for text in generator.templates.values() {
        for piece in pieces(text).unwrap_or_default() {
            if let Piece::Hole {
                name,
                kind,
                instruction,
            } = piece
                && !seen.iter().any(|known| known == name)
            {
                seen.push(name.to_string());
                let filled = if generator.holes.contains_key(name) {
                    "filled from the inputs"
                } else {
                    ""
                };
                rows.push(vec![
                    name.into(),
                    kind.into(),
                    instruction.into(),
                    filled.into(),
                ]);
            }
        }
    }
    rows
}
