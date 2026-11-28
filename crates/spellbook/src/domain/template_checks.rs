//! A template body checked when its generator loads, so a mistake in a loop, a part or a lookup
//! is a load error naming the template line rather than a surprise when someone generates.

use std::collections::BTreeMap;

use sorcerer_core::domain::template_blocks::{
    Node, Path, Scope, Step, holes_in_loops, loops_in, walk_paths,
};

use crate::domain::generator::{Generator, InputKind};
use crate::domain::generator_problems::function_style_text;
use crate::domain::list_items::ItemPattern;
use crate::domain::tables::Rows;

/// What a body may name: the generator (its inputs, list patterns and tables) and the names a
/// template may use (inputs, the built-ins and derived names).
pub struct BodyNames<'g> {
    pub generator: &'g Generator,
    pub known: &'g [&'g str],
}

/// Checks a body's tree: each loop walks a list input under a name of its own, no hole sits
/// inside a loop, and every path names something the generator gives: an item's part, `loop`'s
/// `first`, `last` or `index`, a name, or a table's column looked up by a part `keys_of` checks.
///
/// Example: in a loop `{= for f in fields =}` over items `{name}:{kind}`, `{= f.colour =}` on
/// line 2 is refused with "`f.colour` (line 2): an item of `fields` has the parts name, kind,
/// optional".
pub fn check_body(nodes: &[Node<'_>], names: &BodyNames) -> Result<(), String> {
    loops_walk_list_inputs(nodes, names)?;
    no_hole_inside_a_loop(nodes)?;
    paths_name_what_the_generator_gives(nodes, names)
}

fn loops_walk_list_inputs(nodes: &[Node<'_>], names: &BodyNames) -> Result<(), String> {
    for (item, list, line) in loops_in(nodes) {
        let tag = format!("`{{= for {item} in {list} =}}` (line {line})");
        if let Some(what) = names.taken_name(item) {
            return Err(format!(
                "{tag}: `{item}` is {what}; name the item something else"
            ));
        }
        if names.list_pattern(list).is_none() {
            return Err(format!("{tag}: `{list}` is not a list input"));
        }
    }
    Ok(())
}

fn no_hole_inside_a_loop(nodes: &[Node<'_>]) -> Result<(), String> {
    match holes_in_loops(nodes).first() {
        Some((name, line)) => Err(format!(
            "a hole inside `{{= for =}}` would repeat; move `{{= ? {name} … =}}` (line {line}) outside the loop"
        )),
        None => Ok(()),
    }
}

fn paths_name_what_the_generator_gives(
    nodes: &[Node<'_>],
    names: &BodyNames,
) -> Result<(), String> {
    let mut first_problem = None;
    walk_paths(nodes, &mut |path, scope| {
        if first_problem.is_none() {
            first_problem = check_path(path, &scope, names).err();
        }
    });
    first_problem.map_or(Ok(()), Err)
}

/// What a path's root is decides what may follow it.
fn check_path(path: &Path<'_>, scope: &Scope<'_>, names: &BodyNames) -> Result<(), String> {
    if let Some(list) = list_walked_by(scope, path.root) {
        return item_path(path, list, names);
    }
    if path.root == LOOP && !scope.loops.is_empty() {
        return loop_path(path);
    }
    if names.known.contains(&path.root) {
        return plain_name_path(path, names);
    }
    if let Some(rows) = names.generator.tables.rows(path.root) {
        return table_path(path, rows, scope, names);
    }
    Err(unknown_name_text(path, names.known))
}

/// A name the generator does not give: olifant's function style gets the filter to write,
/// anything else the list of names.
///
/// Example: "`nope` is not an input or a derived name, on line 3 (they are: command, timestamp)".
fn unknown_name_text(path: &Path<'_>, known: &[&str]) -> String {
    function_style_text(path.root).unwrap_or_else(|| {
        format!(
            "`{}` is not an input or a derived name, on line {} (they are: {})",
            path.root,
            path.line,
            known.join(", ")
        )
    })
}

/// The record a loop binds beside its item.
const LOOP: &str = "loop";
const LOOP_PARTS: &[&str] = &["first", "last", "index"];

/// The list the innermost loop with this item name walks.
fn list_walked_by<'a>(scope: &Scope<'a>, item: &str) -> Option<&'a str> {
    scope
        .loops
        .iter()
        .rev()
        .find(|(name, _)| *name == item)
        .map(|(_, list)| *list)
}

/// `f.name`: a part of the item's pattern, or `optional`, and nothing after it.
fn item_path(path: &Path<'_>, list: &str, names: &BodyNames) -> Result<(), String> {
    let Some(pattern) = names.list_pattern(list) else {
        return Ok(());
    };
    let parts = pattern.item_parts();
    match path.steps.first() {
        None => Ok(()),
        Some(Step::Field(part)) if parts.contains(part) => nothing_after(path, 1),
        Some(_) => Err(format!(
            "`{path}` (line {}): an item of `{list}` has the parts {}",
            path.line,
            parts.join(", ")
        )),
    }
}

/// `loop.last`: one of `first`, `last`, `index`.
fn loop_path(path: &Path<'_>) -> Result<(), String> {
    match path.steps.as_slice() {
        [Step::Field(part)] if LOOP_PARTS.contains(part) => Ok(()),
        _ => Err(format!(
            "`{path}` (line {}): `{LOOP}` has the parts {}",
            path.line,
            LOOP_PARTS.join(", ")
        )),
    }
}

/// An input or derived name stands alone: it is text (or, for a list input, a list).
fn plain_name_path(path: &Path<'_>, names: &BodyNames) -> Result<(), String> {
    match path.steps.is_empty() {
        true => Ok(()),
        false => {
            let sort = match names.list_pattern(path.root) {
                Some(_) => "a list",
                None => "text",
            };
            Err(format!(
                "`{path}` (line {}): `{}` is {sort}, it has no parts",
                path.line, path.root
            ))
        }
    }
}

/// `kinds[f.kind].rust`: a lookup by a part `keys_of` checks against this table, then a column
/// every row has.
fn table_path(
    path: &Path<'_>,
    rows: &Rows,
    scope: &Scope<'_>,
    names: &BodyNames,
) -> Result<(), String> {
    let table = path.root;
    let Some(Step::Key(key)) = path.steps.first() else {
        return Err(format!(
            "`{path}` (line {}): `{table}` is a table: look a row up with `{table}[f.part]`",
            path.line
        ));
    };
    key_is_checked_against(key, table, path, scope, names)?;
    let column = match path.steps.get(1) {
        Some(Step::Field(column)) => *column,
        _ => {
            return Err(format!(
                "`{path}` (line {}): a row of `{table}` is read by a column (`{}.column`)",
                path.line,
                path.written_upto(1)
            ));
        }
    };
    if let Some((row, _)) = rows.iter().find(|(_, cells)| !cells.contains_key(column)) {
        return Err(format!(
            "`{path}` (line {}): row `{row}` of table `{table}` has no `{column}`",
            path.line
        ));
    }
    nothing_after(path, 2)
}

/// A lookup's key must be an item's part that `keys_of` checks against the table, so every key
/// the template can meet is a row the table has.
fn key_is_checked_against(
    key: &Path<'_>,
    table: &str,
    path: &Path<'_>,
    scope: &Scope<'_>,
    names: &BodyNames,
) -> Result<(), String> {
    let checked = match (list_walked_by(scope, key.root), key.steps.as_slice()) {
        (Some(list), [Step::Field(part)]) => names
            .keys_of(list)
            .is_some_and(|keys_of| keys_of.get(*part).is_some_and(|known| known == table)),
        _ => false,
    };
    if checked {
        return Ok(());
    }
    let named = match key.steps.last() {
        Some(Step::Field(part)) => (*part).to_string(),
        _ => key.to_string(),
    };
    Err(format!(
        "`{}` (line {}): `{named}` is not checked against `{table}`; look up a part keys_of names",
        path.written_upto(1),
        path.line
    ))
}

/// The steps from `from` on must not be there: what is before them is text.
fn nothing_after(path: &Path<'_>, from: usize) -> Result<(), String> {
    match path.steps.len() > from {
        false => Ok(()),
        true => Err(format!(
            "`{path}` (line {}): `{}` is text, it has no parts",
            path.line,
            path.written_upto(from)
        )),
    }
}

impl BodyNames<'_> {
    /// The item pattern of the list input with this name.
    fn list_pattern(&self, name: &str) -> Option<&ItemPattern> {
        self.list_kind(name).map(|(pattern, _)| pattern)
    }

    /// The `keys_of` of the list input with this name.
    fn keys_of(&self, name: &str) -> Option<&BTreeMap<String, String>> {
        self.list_kind(name).map(|(_, keys_of)| keys_of)
    }

    fn list_kind(&self, name: &str) -> Option<(&ItemPattern, &BTreeMap<String, String>)> {
        self.generator
            .inputs
            .iter()
            .find(|(input, _)| input == name)
            .and_then(|(_, input)| match &input.kind {
                InputKind::List { pattern, keys_of } => Some((pattern, keys_of)),
                _ => None,
            })
    }

    /// What a name already is, when a loop item may not take it: an input, a derived name or
    /// `loop`.
    fn taken_name(&self, name: &str) -> Option<&'static str> {
        if self.generator.inputs.iter().any(|(input, _)| input == name) {
            Some("an input")
        } else if self.known.contains(&name) {
            Some("a name the generator gives")
        } else if name == LOOP {
            Some("the loop's own record")
        } else {
            None
        }
    }
}
