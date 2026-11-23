//! The checks that look across a generator's file: its language is the project's, every enum
//! names a query that only uses earlier inputs, every template parses as a template and uses
//! names the generator gives, and every hole has one kind and one plain instruction, of a kind
//! the language has.

use std::collections::BTreeMap;

use sorcerer_core::domain::language::Languages;
use sorcerer_core::domain::templates::{Piece, pieces};

use crate::domain::generator::{EditBody, Generator, HoleSource, InputKind};
use crate::domain::generator_problems::function_style_text;
use crate::domain::holes_table::{holes_table, missing_kind_text};

/// A problem the checks found: the generator's file it is in, and the sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckProblem {
    pub file: String,
    pub why: String,
}

const GENERATOR_FILE: &str = "generator.toml";

/// Checks a generator read from its file against itself and the project's languages; the first
/// problem found is the error.
///
/// Example: an enum input `service` whose query's glob uses the input `command`, which comes
/// after it, is refused with
/// `[query.services] glob: a query naming an input not yet given (command, before service)`.
pub fn check_generator(generator: &Generator, languages: &Languages) -> Result<(), CheckProblem> {
    in_generator_file(language_is_known(generator, languages))?;
    in_generator_file(enums_name_queries_over_earlier_inputs(generator))?;
    in_generator_file(edit_templates_exist(generator))?;
    in_generator_file(names_and_values_are_plain(generator))?;
    templates_are_well_formed(generator)?;
    holes_agree(generator, languages)
}

fn in_generator_file(result: Result<(), String>) -> Result<(), CheckProblem> {
    result.map_err(|why| CheckProblem {
        file: GENERATOR_FILE.into(),
        why,
    })
}

fn language_is_known(generator: &Generator, languages: &Languages) -> Result<(), String> {
    match languages.get(&generator.language) {
        Some(_) => Ok(()),
        None => Err(format!(
            "language = \"{}\" is not a language of this project (it has: {})",
            generator.language,
            languages.names().join(", ")
        )),
    }
}

fn enums_name_queries_over_earlier_inputs(generator: &Generator) -> Result<(), String> {
    for (position, (name, input)) in generator.inputs.iter().enumerate() {
        let InputKind::Enum { one_of } = &input.kind else {
            continue;
        };
        let query = generator.queries.get(one_of).ok_or_else(|| {
            format!("[inputs.{name}] one_of = \"{one_of}\" names no [query.{one_of}]")
        })?;
        let earlier: Vec<&str> = generator.inputs[..position]
            .iter()
            .map(|(earlier, _)| earlier.as_str())
            .collect();
        for used in value_names(&query.glob, &format!("[query.{one_of}] glob"))? {
            if !earlier.contains(&used) {
                return Err(format!(
                    "[query.{one_of}] glob: a query naming an input not yet given ({used}, before {name})"
                ));
            }
        }
    }
    Ok(())
}

fn edit_templates_exist(generator: &Generator) -> Result<(), String> {
    for (number, edit) in generator.edits.iter().enumerate() {
        if let EditBody::Template(template) = &edit.body
            && !generator.templates.contains_key(template)
        {
            return Err(format!(
                "[[edits]] {}: template \"{template}\" is not a file in templates/",
                number + 1
            ));
        }
    }
    Ok(())
}

/// Paths, derived names and derived holes are templates of values only, over names the
/// generator gives.
fn names_and_values_are_plain(generator: &Generator) -> Result<(), String> {
    let known = known_names(generator);
    let paths = generator
        .edits
        .iter()
        .enumerate()
        .map(|(number, edit)| (format!("[[edits]] {} create", number + 1), &edit.create));
    let derives = generator
        .derive
        .iter()
        .map(|(name, template)| (format!("[derive] {name}"), template));
    let holes = generator.holes.iter().map(|(name, source)| match source {
        HoleSource::Derived { value } => (format!("[holes.{name}] value"), value),
    });
    for (place, template) in paths.chain(derives).chain(holes) {
        for used in value_names(template, &place)? {
            refuse_unknown_name(used, &known, &place)?;
        }
    }
    Ok(())
}

fn templates_are_well_formed(generator: &Generator) -> Result<(), CheckProblem> {
    let known = known_names(generator);
    for (file, text) in &generator.templates {
        let in_template = |why: String| CheckProblem {
            file: format!("templates/{file}"),
            why,
        };
        for piece in pieces(text).map_err(in_template)? {
            if let Piece::Value { name, .. } = piece {
                refuse_unknown_name(name, &known, "the template").map_err(in_template)?;
            }
        }
    }
    Ok(())
}

/// Every hole name has one kind and one instruction across the templates; the instruction is
/// plain text; a hole no `[holes.X]` fills has a kind the generator's language has.
fn holes_agree(generator: &Generator, languages: &Languages) -> Result<(), CheckProblem> {
    let mut seen: BTreeMap<&str, (&str, &str, &str)> = BTreeMap::new();
    for (file, text) in &generator.templates {
        let in_template = |why: String| CheckProblem {
            file: format!("templates/{file}"),
            why,
        };
        for piece in pieces(text).map_err(in_template)? {
            let Piece::Hole {
                name,
                kind,
                instruction,
            } = piece
            else {
                continue;
            };
            instruction_is_plain(name, instruction).map_err(in_template)?;
            match seen.get(name) {
                Some((first_file, first_kind, first_instruction))
                    if (*first_kind, *first_instruction) != (kind, instruction) =>
                {
                    return Err(in_template(two_ways_text(name, first_file)));
                }
                Some(_) => {}
                None => {
                    if !generator.holes.contains_key(name) {
                        kind_is_known(generator, languages, kind).map_err(in_template)?;
                    }
                    seen.insert(name, (file, kind, instruction));
                }
            }
        }
    }
    Ok(())
}

fn kind_is_known(generator: &Generator, languages: &Languages, kind: &str) -> Result<(), String> {
    let Some(language) = languages.get(&generator.language) else {
        return Ok(());
    };
    let table = holes_table(language).map_err(|e| e.to_string())?;
    match table.kinds.contains_key(kind) {
        true => Ok(()),
        false => Err(missing_kind_text(&language.name, kind, &table)),
    }
}

/// A placeholder puts the instruction inside a string literal, so it may not hold a quote, a
/// backslash, a new line or another template.
fn instruction_is_plain(name: &str, instruction: &str) -> Result<(), String> {
    let refused = ['"', '\\', '\n', '\r'];
    if instruction.contains(refused) || instruction.contains("{=") {
        Err(format!(
            "the hole {name}'s instruction must be plain text: no quote, backslash, new line or {{= =}}"
        ))
    } else {
        Ok(())
    }
}

fn two_ways_text(name: &str, first_file: &str) -> String {
    format!(
        "the hole {name} is written with another kind or instruction in templates/{first_file}; \
         one hole has one kind and one instruction"
    )
}

/// The names a template may use: the inputs and the derived names.
fn known_names(generator: &Generator) -> Vec<&str> {
    generator
        .inputs
        .iter()
        .map(|(name, _)| name.as_str())
        .chain(generator.derive.iter().map(|(name, _)| name.as_str()))
        .collect()
}

/// The value names a template of values uses; a hole there is an error.
fn value_names<'t>(template: &'t str, place: &str) -> Result<Vec<&'t str>, String> {
    pieces(template)
        .map_err(|why| format!("{place}: {why}"))?
        .into_iter()
        .filter_map(|piece| match piece {
            Piece::Text(_) => None,
            Piece::Value { name, .. } => Some(Ok(name)),
            Piece::Hole { name, .. } => Some(Err(format!(
                "{place}: the hole {name} belongs in a template file, not here"
            ))),
        })
        .collect()
}

fn refuse_unknown_name(name: &str, known: &[&str], place: &str) -> Result<(), String> {
    if known.contains(&name) {
        return Ok(());
    }
    let why = function_style_text(name).unwrap_or_else(|| {
        format!(
            "`{name}` is not an input or a derived name (they are: {})",
            known.join(", ")
        )
    });
    Err(format!("{place}: {why}"))
}
