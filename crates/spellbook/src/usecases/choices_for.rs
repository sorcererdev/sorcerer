//! Inputs resolved in the generator's order, and the choices an enum input has now.

use sorcerer_core::domain::templates::fill;
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::generator::{Generator, Input, InputKind};
use crate::domain::inputs::{Given, InputStop, Values, is_a_name, query_choices};

/// Every input had, then the derived names; or why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    Ready(Values),
    Stop(InputStop),
}

/// Resolves the generator's inputs in its order, then its derived names. Each input is the value
/// given (an enum's checked against its choices, an ident's against what a name is); when none
/// is given, an enum with one choice takes it, then the input's `default`, then empty for an
/// optional one; otherwise the generation stops asking for it, with its choices.
///
/// Example: in bistro, with `service=booking` and `command=cancel_table` given, `app` is
/// `bistro` (the only folder under `src/` holding `main.py`).
pub fn resolve_inputs(view: &dyn CodebaseView, generator: &Generator, given: &Given) -> Resolved {
    match resolve_up_to(view, generator, given, generator.inputs.len())
        .and_then(|values| values.with_derived(generator).map_err(InputStop::Template))
    {
        Ok(values) => Resolved::Ready(values),
        Err(stop) => Resolved::Stop(stop),
    }
}

/// The values an input may take now: an enum's choices from the project, after the inputs
/// before it are resolved; nothing for an input of another kind.
///
/// Example: in bistro with a `kitchen/models/` folder added, `service` has
/// `["booking", "kitchen"]`.
pub fn choices_for(
    view: &dyn CodebaseView,
    generator: &Generator,
    given: &Given,
    input: &str,
) -> Result<Vec<String>, InputStop> {
    let position = generator
        .inputs
        .iter()
        .position(|(name, _)| name == input)
        .ok_or_else(|| InputStop::Template(format!("the generator has no input {input}")))?;
    let before = resolve_up_to(view, generator, given, position)?;
    enum_choices(
        view,
        generator,
        &before,
        input,
        &generator.inputs[position].1,
    )
}

/// The first `count` inputs resolved, in order.
fn resolve_up_to(
    view: &dyn CodebaseView,
    generator: &Generator,
    given: &Given,
    count: usize,
) -> Result<Values, InputStop> {
    generator.inputs[..count]
        .iter()
        .try_fold(Values::default(), |values, (name, input)| {
            let value = resolve_one(view, generator, given, &values, name, input)?;
            let mut entries: Vec<(String, String)> = values
                .pairs()
                .into_iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect();
            entries.push((name.clone(), value));
            Ok(Values::of_inputs(entries))
        })
}

fn resolve_one(
    view: &dyn CodebaseView,
    generator: &Generator,
    given: &Given,
    before: &Values,
    name: &str,
    input: &Input,
) -> Result<String, InputStop> {
    let choices = enum_choices(view, generator, before, name, input)?;
    match given.get(name) {
        Some(value) => checked(name, value, input, choices),
        None => untold(name, input, choices),
    }
}

/// A value given: an enum's must be a choice, an ident's a name.
fn checked(
    name: &str,
    value: &str,
    input: &Input,
    choices: Vec<String>,
) -> Result<String, InputStop> {
    match &input.kind {
        InputKind::Enum { .. } if !choices.iter().any(|choice| choice == value) => {
            Err(InputStop::NotAChoice {
                input: name.into(),
                given: value.into(),
                choices,
            })
        }
        InputKind::Ident if !is_a_name(value) => Err(InputStop::NotAName {
            input: name.into(),
            given: value.into(),
        }),
        _ => Ok(value.to_string()),
    }
}

/// No value given: one choice, the default, empty when optional, else the question.
fn untold(name: &str, input: &Input, choices: Vec<String>) -> Result<String, InputStop> {
    if let [only] = choices.as_slice() {
        return Ok(only.clone());
    }
    if let Some(default) = &input.default {
        return Ok(default.clone());
    }
    if input.optional {
        return Ok(String::new());
    }
    Err(InputStop::Missing {
        input: name.into(),
        prompt: input.prompt.clone(),
        choices,
    })
}

/// An enum's choices now; none for another kind. A query that finds nothing stops.
fn enum_choices(
    view: &dyn CodebaseView,
    generator: &Generator,
    before: &Values,
    name: &str,
    input: &Input,
) -> Result<Vec<String>, InputStop> {
    let InputKind::Enum { one_of } = &input.kind else {
        return Ok(Vec::new());
    };
    let query = generator
        .queries
        .get(one_of)
        .ok_or_else(|| InputStop::Template(format!("[query.{one_of}] is missing")))?;
    let glob = fill(&query.glob, &before.pairs())
        .map_err(|why| InputStop::Template(format!("[query.{one_of}] glob: {why}")))?;
    let files = view
        .glob("**")
        .map_err(|e| InputStop::Template(format!("the project's files: {e}")))?;
    let choices = query_choices(&files, &glob, query.value)
        .map_err(|why| InputStop::Template(format!("[query.{one_of}] {why}")))?;
    if choices.is_empty() {
        return Err(InputStop::NoChoices {
            input: name.into(),
            query: one_of.clone(),
        });
    }
    Ok(choices)
}
