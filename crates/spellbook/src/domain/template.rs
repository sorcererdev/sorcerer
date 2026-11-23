//! A template rendered: values filled (with their filters), holes `[holes.X]` derives filled
//! from the inputs, and every other hole replaced by its kind's placeholder, each hole's line
//! remembered.

use std::collections::BTreeMap;

use sorcerer_core::domain::templates::{Piece, fill, filled_value, pieces};

use crate::domain::generator::HoleSource;
use crate::domain::holes::{HoleAt, HoleOrigin};
use crate::domain::inputs::Values;

/// A template's text once rendered, and where each hole landed (line from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    pub holes: Vec<HoleAt>,
}

/// Renders a template. A hole `derived` names is filled from its value template; any other is
/// replaced by `placeholder_of(kind, name, instruction)`, which sits where the hole sat, so a
/// block placeholder keeps the indentation the template gave the hole.
///
/// Example: `    {= ? handle: block | its first case =}` with a placeholder of
/// `fail("hole handle: its first case")` renders as `    fail("hole handle: its first case")`,
/// and the hole is remembered at that line.
pub fn render(
    template: &str,
    values: &Values,
    derived: &BTreeMap<String, HoleSource>,
    placeholder_of: &dyn Fn(&str, &str, &str) -> Result<String, String>,
) -> Result<Rendered, String> {
    let pairs = values.pairs();
    let mut text = String::new();
    let mut holes: Vec<HoleAt> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    for piece in pieces(template)? {
        match piece {
            Piece::Text(plain) => text.push_str(plain),
            Piece::Value { name, filters } => {
                text.push_str(&filled_value(name, &filters, &pairs, template)?);
            }
            Piece::Hole {
                name,
                kind,
                instruction,
            } => {
                let (filled, source) = match derived.get(name) {
                    Some(HoleSource::Derived { value }) => {
                        (fill(value, &pairs)?, HoleOrigin::Derived)
                    }
                    None => (
                        placeholder_of(kind, name, instruction)?,
                        HoleOrigin::Placeholder,
                    ),
                };
                let before = text.rsplit('\n').next().unwrap_or("");
                let edge_before = before.chars().rev().find(|c| !c.is_whitespace());
                holes.push(HoleAt {
                    name: name.into(),
                    kind: kind.into(),
                    instruction: instruction.into(),
                    source,
                    line: line_after(&text),
                    text: filled.clone(),
                    edges: edge_before.map(String::from).unwrap_or_else(|| "\n".into()),
                });
                text.push_str(&filled);
                ends.push(text.len());
            }
        }
    }
    for (hole, end) in holes.iter_mut().zip(ends) {
        let rest = text[end..].split('\n').next().unwrap_or("");
        hole.edges
            .push(rest.chars().find(|c| !c.is_whitespace()).unwrap_or('\n'));
    }
    Ok(Rendered { text, holes })
}

/// The line (from 1) the next character written after this text lands on.
fn line_after(text: &str) -> u32 {
    u32::try_from(text.matches('\n').count()).unwrap_or(u32::MAX - 1) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Values {
        Values::of_inputs(vec![("command".into(), "cancel_table".into())])
    }

    fn placeholder(kind: &str, name: &str, instruction: &str) -> Result<String, String> {
        Ok(match kind {
            "ident" => format!("todo_{name}"),
            _ => format!("fail(\"hole {name}: {instruction}\")"),
        })
    }

    fn derived() -> BTreeMap<String, HoleSource> {
        BTreeMap::from([(
            "purpose".to_string(),
            HoleSource::Derived {
                value: "{= command|sentence =}.".into(),
            },
        )])
    }

    #[test]
    fn values_and_filters_render() {
        let rendered = render(
            "class {= command|pascal =}: {= command =}",
            &values(),
            &derived(),
            &placeholder,
        )
        .unwrap();
        assert_eq!(rendered.text, "class CancelTable: cancel_table");
        assert!(rendered.holes.is_empty());
    }

    #[test]
    fn a_derived_hole_is_filled_from_the_inputs() {
        let rendered = render(
            "\"{= ? purpose: text | what it does =}\"",
            &values(),
            &derived(),
            &placeholder,
        )
        .unwrap();
        assert_eq!(rendered.text, "\"Cancel table.\"");
        assert_eq!(rendered.holes[0].source, HoleOrigin::Derived);
    }

    #[test]
    fn a_placeholder_carries_name_and_instruction() {
        let rendered = render(
            "{= ? handle: block | its first case =}",
            &values(),
            &derived(),
            &placeholder,
        )
        .unwrap();
        assert_eq!(rendered.text, "fail(\"hole handle: its first case\")");
        assert_eq!(rendered.holes[0].source, HoleOrigin::Placeholder);
        assert_eq!(rendered.holes[0].instruction, "its first case");
    }

    #[test]
    fn the_hole_line_is_where_the_placeholder_lands() {
        let template = "\"{= ? purpose: text | x =}\"\n\na\nb {= ? field: ident | y =}\n";
        let rendered = render(template, &values(), &derived(), &placeholder).unwrap();
        assert_eq!(rendered.holes[0].line, 1);
        assert_eq!(rendered.holes[1].line, 4);
        assert_eq!(rendered.text.lines().nth(3), Some("b todo_field"));
    }

    #[test]
    fn a_block_hole_keeps_its_indentation() {
        let template = "def f():\n        {= ? handle: block | x =}\n";
        let rendered = render(template, &values(), &derived(), &placeholder).unwrap();
        assert_eq!(
            rendered.text,
            "def f():\n        fail(\"hole handle: x\")\n"
        );
        assert_eq!(rendered.holes[0].line, 2);
    }
}
