//! Inputs resolved in the generator's order, and the choices an enum input has now.

use sorcerer_core::domain::templates::fill;
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::generator::{Generator, Input, InputKind};
use crate::domain::inputs::{Given, InputStop, Values, is_a_name, query_choices};
use crate::domain::list_items::ListItem;

/// Every input had, then the derived names; or why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    Ready(Values),
    Stop(InputStop),
}

/// Resolves the generator's inputs in its order, then the built-in `timestamp` (when it was
/// given: the plan sets it from its clock), then its derived names. Each input is the value
/// given (an enum's checked against its choices, an ident's against what a name is); when none
/// is given, an enum with one choice takes it, then the input's `default`, then empty for an
/// optional one; otherwise the generation stops asking for it, with its choices.
///
/// Example: in bistro, with `service=booking` and `command=cancel_table` given, `app` is
/// `bistro` (the only folder under `src/` holding `main.py`).
pub fn resolve_inputs(view: &dyn CodebaseView, generator: &Generator, given: &Given) -> Resolved {
    match resolve_up_to(view, generator, given, generator.inputs.len())
        .map(|values| values.with_built_ins(generator, given))
        .map(|values| values.with_tables(&generator.tables))
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
    if let InputKind::Fixed { choices } = &generator.inputs[position].1.kind {
        // The list is the generator's own: no earlier input is needed to know it.
        return Ok(choices.clone());
    }
    let before = resolve_up_to(view, generator, given, position)?;
    enum_choices(
        view,
        generator,
        &before,
        input,
        &generator.inputs[position].1,
    )
}

/// The first `count` inputs resolved, in order; a list input is read into its items too.
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
            let values = values.with_input(name, &value);
            match list_items(generator, name, input, &value)? {
                Some(items) => Ok(values.with_list(name, items)),
                None => Ok(values),
            }
        })
}

/// A list input's value read into its items by its pattern; `None` for another kind. An item
/// that does not follow the pattern stops, naming the input, the item and why.
fn list_items(
    generator: &Generator,
    name: &str,
    input: &Input,
    value: &str,
) -> Result<Option<Vec<ListItem>>, InputStop> {
    let InputKind::List { pattern, keys_of } = &input.kind else {
        return Ok(None);
    };
    pattern
        .items(value, keys_of, &generator.tables)
        .map(Some)
        .map_err(|problem| InputStop::BadItem {
            input: name.into(),
            item: problem.item,
            why: problem.why,
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
        InputKind::Enum { .. } | InputKind::Fixed { .. }
            if !choices.iter().any(|choice| choice == value) =>
        {
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

/// An enum's choices now (a fixed list as written); none for another kind. A query that finds nothing stops.
fn enum_choices(
    view: &dyn CodebaseView,
    generator: &Generator,
    before: &Values,
    name: &str,
    input: &Input,
) -> Result<Vec<String>, InputStop> {
    let one_of = match &input.kind {
        InputKind::Enum { one_of } => one_of,
        InputKind::Fixed { choices } => return Ok(choices.clone()),
        _ => return Ok(Vec::new()),
    };
    let query = generator
        .queries
        .get(one_of)
        .ok_or_else(|| InputStop::Template(format!("[query.{one_of}] is missing")))?;
    let glob = fill(&query.glob, &before.pairs())
        .map_err(|why| InputStop::Template(format!("[query.{one_of}] glob: {why}")))?;
    let exclude = query
        .exclude
        .iter()
        .map(|pattern| fill(pattern, &before.pairs()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|why| InputStop::Template(format!("[query.{one_of}] exclude: {why}")))?;
    let files = view
        .glob("**")
        .map_err(|e| InputStop::Template(format!("the project's files: {e}")))?;
    let choices = query_choices(&files, &glob, &exclude, query.value)
        .map_err(|why| InputStop::Template(format!("[query.{one_of}] {why}")))?;
    if choices.is_empty() {
        return Err(InputStop::NoChoices {
            input: name.into(),
            query: one_of.clone(),
        });
    }
    Ok(choices)
}
