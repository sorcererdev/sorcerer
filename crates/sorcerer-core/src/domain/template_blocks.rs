//! Template bodies that loop and branch: `{= for f in fields =} … {= end =}` and
//! `{= if … =} … {= elif … =} … {= else =} … {= end =}`, read into a tree ([`parse_body`]) and
//! expanded against typed values ([`expand`]) into text and holes.
//!
//! One-line templates (paths, names, notes) stay with the flat reader in
//! [`crate::domain::templates`], which refuses these tags: only a file's body may loop.
//!
//! A line that holds only one control tag (`for`, `if`, `elif`, `else`, `end`) and spaces is
//! removed whole, its newline too, so a loop over three items writes exactly three lines.

use std::borrow::Cow;

use crate::domain::name_forms::apply_filter;
pub use crate::domain::template_conditions::{Condition, Operand};
use crate::domain::template_conditions::{decide, parse_condition, paths_of};
pub use crate::domain::template_paths::{Bindings, Datum, Path, Step};
use crate::domain::template_paths::{Lookup, parse_path};
use crate::domain::templates::{excerpt, hole_parts};

/// One part of a template body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node<'t> {
    /// Text copied as it is (borrowed, trimmed where a control-only line was removed).
    Text(Cow<'t, str>),
    /// `{= path | filter | filter =}`: a value, its filters applied left to right.
    Value {
        path: Path<'t>,
        filters: Vec<&'t str>,
    },
    /// `{= ? name: kind | instruction =}`: a hole, on its template line (from 1).
    Hole {
        name: &'t str,
        kind: &'t str,
        instruction: &'t str,
        line: usize,
    },
    /// `{= for item in list =} body {= end =}`.
    For {
        item: &'t str,
        list: &'t str,
        line: usize,
        body: Vec<Node<'t>>,
    },
    /// `{= if … =}`, any `{= elif … =}`, an optional `{= else =}`, then `{= end =}`.
    If {
        branches: Vec<(Condition<'t>, Vec<Node<'t>>)>,
        otherwise: Vec<Node<'t>>,
        line: usize,
    },
}

/// Reads a template body into its tree. Errors name the tag and its line (from 1): a `for`
/// or `if` with no `end`, an `end` that closes nothing, an `else` or `elif` outside an `if`, a
/// condition that does not read, a value that is not a path.
///
/// Example: `parse_body("{= for f in fields =}{= f.name =} {= end =}")` is one `For` over
/// `fields` whose body is the value `f.name` and the text `" "`.
pub fn parse_body(template: &str) -> Result<Vec<Node<'_>>, String> {
    let shown = excerpt(template);
    let raw = raw_parts(template, &shown)?;
    let tags = raw
        .iter()
        .map(|part| tag_of(part, &shown))
        .collect::<Result<Vec<_>, _>>()?;
    let alone = control_only_lines(template, &raw, &tags);
    let texts = trimmed_texts(template, &raw, &alone);
    tree_of(tags, texts)
}

/// A template body expanded: text, and the holes left for a person or a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expanded<'t> {
    Text(String),
    Hole {
        name: &'t str,
        kind: &'t str,
        instruction: &'t str,
    },
}

/// Expands a body's tree against the values: loops repeat their body once per item, each `if`
/// keeps its first branch that holds, values are filled with their filters. Text next to text
/// is joined. A name with no value, a loop over something that is not a list, or a part an item
/// lacks is an error naming the template line.
///
/// Example: `{= for f in fields =}{= f.name =}{= if not loop.last =}, {= end =}{= end =}` over
/// items named `name`, `price` gives the text `name, price`.
pub fn expand<'t>(
    nodes: &[Node<'t>],
    bindings: &Bindings,
    template: &str,
) -> Result<Vec<Expanded<'t>>, String> {
    let shown = excerpt(template);
    expand_nodes(nodes, &Lookup::new(bindings), &shown, Vec::new())
}

/// Which loops are in force where a path sits: each loop's item name and the list it walks,
/// outermost first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope<'a> {
    pub loops: Vec<(&'a str, &'a str)>,
}

/// Calls `visit` on every path a body reads (values, conditions, and the paths inside a
/// lookup's brackets), with the loops in force there.
///
/// Example: in `{= for f in fields =}{= kinds[f.kind].rust =}{= end =}` it visits
/// `kinds[f.kind].rust` and then `f.kind`, both with the loop `(f, fields)`.
pub fn walk_paths<'a, 't>(nodes: &'a [Node<'t>], visit: &mut dyn FnMut(&'a Path<'t>, Scope<'a>)) {
    walk_in_scope(nodes, &Scope::default(), visit);
}

/// Every hole inside a loop, by name and template line: a hole there would repeat.
///
/// Example: `{= for f in fields =}\n{= ? check: block =}\n{= end =}` gives `[("check", 2)]`.
pub fn holes_in_loops<'t>(nodes: &[Node<'t>]) -> Vec<(&'t str, usize)> {
    nodes
        .iter()
        .flat_map(|node| match node {
            Node::For { body, .. } => all_holes(body),
            Node::If {
                branches,
                otherwise,
                ..
            } => branches
                .iter()
                .flat_map(|(_, body)| holes_in_loops(body))
                .chain(holes_in_loops(otherwise))
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// Every hole in a body, in the order the template holds them, those inside loops and branches
/// included.
///
/// Example: `a{= if x =}{= ? check: block =}{= end =}` gives the one `Node::Hole` named `check`.
pub fn holes_in<'a, 't>(nodes: &'a [Node<'t>]) -> Vec<&'a Node<'t>> {
    nodes
        .iter()
        .flat_map(|node| match node {
            Node::Hole { .. } => vec![node],
            Node::For { body, .. } => holes_in(body),
            Node::If {
                branches,
                otherwise,
                ..
            } => branches
                .iter()
                .flat_map(|(_, body)| holes_in(body))
                .chain(holes_in(otherwise))
                .collect(),
            Node::Text(_) | Node::Value { .. } => Vec::new(),
        })
        .collect()
}

/// Every loop in a body, outer before inner: its item name, its list name and its line.
///
/// Example: `{= for f in fields =}{= end =}` gives `[("f", "fields", 1)]`.
pub fn loops_in<'t>(nodes: &[Node<'t>]) -> Vec<(&'t str, &'t str, usize)> {
    nodes
        .iter()
        .flat_map(|node| match node {
            Node::For {
                item,
                list,
                line,
                body,
            } => [vec![(*item, *list, *line)], loops_in(body)].concat(),
            Node::If {
                branches,
                otherwise,
                ..
            } => branches
                .iter()
                .flat_map(|(_, body)| loops_in(body))
                .chain(loops_in(otherwise))
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// A stretch of the template: text, or what sits between `{=` and `=}` (trimmed), with byte
/// positions and the tag's line.
#[derive(Debug, Clone, Copy)]
enum Raw<'t> {
    Text { start: usize, end: usize },
    Tag { inside: &'t str, line: usize },
}

/// A tag read, or `None` for text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tag<'t> {
    Value {
        path: Path<'t>,
        filters: Vec<&'t str>,
    },
    Hole {
        name: &'t str,
        kind: &'t str,
        instruction: &'t str,
        line: usize,
    },
    For {
        item: &'t str,
        list: &'t str,
        line: usize,
    },
    If {
        condition: Condition<'t>,
        line: usize,
    },
    Elif {
        condition: Condition<'t>,
        line: usize,
    },
    Else {
        line: usize,
    },
    End {
        line: usize,
    },
}

impl Tag<'_> {
    fn is_control(&self) -> bool {
        !matches!(self, Tag::Value { .. } | Tag::Hole { .. })
    }
}

fn raw_parts<'t>(template: &'t str, shown: &str) -> Result<Vec<Raw<'t>>, String> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = template[from..].find("{=").map(|at| from + at) {
        if open > from {
            found.push(Raw::Text {
                start: from,
                end: open,
            });
        }
        let close = template[open + 2..]
            .find("=}")
            .map(|at| open + 2 + at)
            .ok_or_else(|| format!("`{{=` is never closed in `{shown}`"))?;
        let line = template[..open].matches('\n').count() + 1;
        found.push(Raw::Tag {
            inside: template[open + 2..close].trim(),
            line,
        });
        from = close + 2;
    }
    if from < template.len() {
        found.push(Raw::Text {
            start: from,
            end: template.len(),
        });
    }
    Ok(found)
}

fn tag_of<'t>(part: &Raw<'t>, shown: &str) -> Result<Option<Tag<'t>>, String> {
    let Raw::Tag { inside, line } = *part else {
        return Ok(None);
    };
    let first_word = inside.split_whitespace().next().unwrap_or_default();
    let rest = inside[first_word.len()..].trim();
    let tag = match first_word {
        _ if inside.starts_with('?') => {
            let (name, kind, instruction) = hole_parts(inside[1..].trim(), shown)?;
            Tag::Hole {
                name,
                kind,
                instruction,
                line,
            }
        }
        "for" => for_tag(inside, rest, line)?,
        "if" => Tag::If {
            condition: parse_condition(rest, inside, line)?,
            line,
        },
        "elif" => Tag::Elif {
            condition: parse_condition(rest, inside, line)?,
            line,
        },
        "else" if rest.is_empty() => Tag::Else { line },
        "end" if rest.is_empty() => Tag::End { line },
        _ => value_tag(inside, line, shown)?,
    };
    Ok(Some(tag))
}

fn for_tag<'t>(inside: &str, rest: &'t str, line: usize) -> Result<Tag<'t>, String> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    match words[..] {
        [item, "in", list] if is_a_plain_name(item) && is_a_plain_name(list) => {
            Ok(Tag::For { item, list, line })
        }
        _ => Err(format!(
            "`{{= {inside} =}}` (line {line}): write a loop as `{{= for item in list =}}`"
        )),
    }
}

fn is_a_plain_name(text: &str) -> bool {
    parse_path(text, 0).is_some_and(|path| path.steps.is_empty())
}

fn value_tag<'t>(inside: &'t str, line: usize, shown: &str) -> Result<Tag<'t>, String> {
    let mut parts = inside.split('|').map(str::trim);
    let written = parts.next().unwrap_or_default();
    if written.is_empty() {
        return Err(format!("a `{{= =}}` with no name (in `{shown}`)"));
    }
    let path = parse_path(written, line).ok_or_else(|| {
        format!("`{{= {inside} =}}` (line {line}): `{written}` is not a name or a path")
    })?;
    Ok(Tag::Value {
        path,
        filters: parts.collect(),
    })
}

/// For each part, whether it is a control tag alone on its line (spaces aside).
fn control_only_lines(template: &str, raw: &[Raw<'_>], tags: &[Option<Tag<'_>>]) -> Vec<bool> {
    let last = raw.len().saturating_sub(1);
    let text_of = |at: usize| match raw.get(at) {
        Some(Raw::Text { start, end }) => Some(&template[*start..*end]),
        _ => None,
    };
    let is_blank = |text: &str| text.chars().all(char::is_whitespace);
    let clear_before = |at: usize| {
        at == 0
            || text_of(at - 1).is_some_and(|text| match text.rfind('\n') {
                Some(newline) => is_blank(&text[newline + 1..]),
                None => at - 1 == 0 && is_blank(text),
            })
    };
    let clear_after = |at: usize| {
        at == last
            || text_of(at + 1).is_some_and(|text| match text.find('\n') {
                Some(newline) => is_blank(&text[..newline]),
                None => at + 1 == last && is_blank(text),
            })
    };
    tags.iter()
        .enumerate()
        .map(|(at, tag)| {
            tag.as_ref().is_some_and(Tag::is_control) && clear_before(at) && clear_after(at)
        })
        .collect()
}

/// Each text part, cut back where a control-only line beside it is removed: the spaces before
/// the tag on its line, and the spaces and newline after it.
fn trimmed_texts<'t>(template: &'t str, raw: &[Raw<'_>], alone: &[bool]) -> Vec<Option<&'t str>> {
    raw.iter()
        .enumerate()
        .map(|(at, part)| {
            let Raw::Text { start, end } = *part else {
                return None;
            };
            let text = &template[start..end];
            let cut_end = match alone.get(at + 1) {
                Some(true) => text.rfind('\n').map_or(0, |newline| newline + 1),
                _ => text.len(),
            };
            let cut_start = match at.checked_sub(1).map(|before| alone[before]) {
                Some(true) => text.find('\n').map_or(text.len(), |newline| newline + 1),
                _ => 0,
            };
            Some(&text[cut_start.min(cut_end)..cut_end])
        })
        .collect()
}

/// A block still open while the tree is built.
enum Open<'t> {
    Top,
    For {
        item: &'t str,
        list: &'t str,
        line: usize,
    },
    If {
        branches: Vec<(Condition<'t>, Vec<Node<'t>>)>,
        /// The condition the body being read belongs to; `None` once past `{= else =}`.
        current: Option<Condition<'t>>,
        line: usize,
    },
}

fn tree_of<'t>(
    tags: Vec<Option<Tag<'t>>>,
    texts: Vec<Option<&'t str>>,
) -> Result<Vec<Node<'t>>, String> {
    let mut stack: Vec<(Open<'t>, Vec<Node<'t>>)> = vec![(Open::Top, Vec::new())];
    for (tag, text) in tags.into_iter().zip(texts) {
        match (tag, text) {
            (None, Some(text)) if !text.is_empty() => {
                body_of(&mut stack).push(Node::Text(Cow::Borrowed(text)));
            }
            (None, _) => {}
            (Some(tag), _) => place_tag(&mut stack, tag)?,
        }
    }
    match stack.pop() {
        Some((Open::Top, nodes)) if stack.is_empty() => Ok(nodes),
        Some((open, _)) => Err(unclosed_text(&open)),
        None => unreachable!("the top block is never closed"),
    }
}

fn body_of<'s, 't>(stack: &'s mut [(Open<'t>, Vec<Node<'t>>)]) -> &'s mut Vec<Node<'t>> {
    &mut stack.last_mut().expect("the top block is never closed").1
}

fn place_tag<'t>(stack: &mut Vec<(Open<'t>, Vec<Node<'t>>)>, tag: Tag<'t>) -> Result<(), String> {
    match tag {
        Tag::Value { path, filters } => body_of(stack).push(Node::Value { path, filters }),
        Tag::Hole {
            name,
            kind,
            instruction,
            line,
        } => {
            body_of(stack).push(Node::Hole {
                name,
                kind,
                instruction,
                line,
            });
        }
        Tag::For { item, list, line } => stack.push((Open::For { item, list, line }, Vec::new())),
        Tag::If { condition, line } => stack.push((
            Open::If {
                branches: Vec::new(),
                current: Some(condition),
                line,
            },
            Vec::new(),
        )),
        Tag::Elif { condition, line } => next_branch(stack, Some(condition), "elif", line)?,
        Tag::Else { line } => next_branch(stack, None, "else", line)?,
        Tag::End { line } => close_block(stack, line)?,
    }
    Ok(())
}

/// Starts the next branch of the open `if`: an `elif` with its condition, or the `else`.
fn next_branch<'t>(
    stack: &mut [(Open<'t>, Vec<Node<'t>>)],
    condition: Option<Condition<'t>>,
    word: &str,
    line: usize,
) -> Result<(), String> {
    let outside = || format!("`{{= {word} =}}` (line {line}) is outside an `{{= if =}}`");
    let (open, body) = stack.last_mut().ok_or_else(outside)?;
    let Open::If {
        branches, current, ..
    } = open
    else {
        return Err(outside());
    };
    let finished = current
        .take()
        .ok_or_else(|| format!("`{{= {word} =}}` (line {line}) comes after the `{{= else =}}`"))?;
    branches.push((finished, std::mem::take(body)));
    *current = condition;
    Ok(())
}

fn close_block<'t>(stack: &mut Vec<(Open<'t>, Vec<Node<'t>>)>, line: usize) -> Result<(), String> {
    let (open, body) = stack.pop().expect("the top block is never closed");
    let node = match open {
        Open::Top => return Err(format!("`{{= end =}}` (line {line}) closes nothing")),
        Open::For { item, list, line } => Node::For {
            item,
            list,
            line,
            body,
        },
        Open::If {
            mut branches,
            current,
            line,
        } => match current {
            Some(condition) => {
                branches.push((condition, body));
                Node::If {
                    branches,
                    otherwise: Vec::new(),
                    line,
                }
            }
            None => Node::If {
                branches,
                otherwise: body,
                line,
            },
        },
    };
    body_of(stack).push(node);
    Ok(())
}

fn unclosed_text(open: &Open<'_>) -> String {
    match open {
        Open::For { item, list, line } => {
            format!("`{{= for {item} in {list} =}}` (line {line}) has no `{{= end =}}`")
        }
        Open::If { line, .. } => format!("the `{{= if =}}` on line {line} has no `{{= end =}}`"),
        Open::Top => unreachable!("the top block is closed by the end of the text"),
    }
}

fn expand_nodes<'t>(
    nodes: &[Node<'t>],
    lookup: &Lookup<'_>,
    shown: &str,
    done: Vec<Expanded<'t>>,
) -> Result<Vec<Expanded<'t>>, String> {
    nodes
        .iter()
        .try_fold(done, |done, node| expand_node(node, lookup, shown, done))
}

fn expand_node<'t>(
    node: &Node<'t>,
    lookup: &Lookup<'_>,
    shown: &str,
    done: Vec<Expanded<'t>>,
) -> Result<Vec<Expanded<'t>>, String> {
    match node {
        Node::Text(text) => Ok(with_text(done, text)),
        Node::Value { path, filters } => {
            Ok(with_text(done, &value_text(path, filters, lookup, shown)?))
        }
        Node::Hole {
            name,
            kind,
            instruction,
            ..
        } => {
            let mut done = done;
            done.push(Expanded::Hole {
                name,
                kind,
                instruction,
            });
            Ok(done)
        }
        Node::For {
            item,
            list,
            line,
            body,
        } => {
            let items = loop_items(item, list, *line, lookup, shown)?;
            let count = items.len();
            items
                .into_iter()
                .enumerate()
                .try_fold(done, |done, (at, datum)| {
                    expand_nodes(
                        body,
                        &lookup.inside_loop(item, datum, at, count),
                        shown,
                        done,
                    )
                })
        }
        Node::If {
            branches,
            otherwise,
            ..
        } => {
            let taken = taken_branch(branches, lookup, shown)?.unwrap_or(otherwise);
            expand_nodes(taken, lookup, shown, done)
        }
    }
}

fn with_text<'t>(mut done: Vec<Expanded<'t>>, text: &str) -> Vec<Expanded<'t>> {
    match done.last_mut() {
        Some(Expanded::Text(before)) => before.push_str(text),
        _ if text.is_empty() => {}
        _ => done.push(Expanded::Text(text.to_string())),
    }
    done
}

fn value_text(
    path: &Path<'_>,
    filters: &[&str],
    lookup: &Lookup<'_>,
    shown: &str,
) -> Result<String, String> {
    let line = path.line;
    let text = match lookup.resolve(path, shown)? {
        Datum::Text(text) => text.clone(),
        Datum::Flag(_) => {
            return Err(format!(
                "`{path}` (line {line}) is a flag: test it with `if` or `not`"
            ));
        }
        Datum::List(_) => {
            return Err(format!(
                "`{path}` (line {line}) is a list: loop over it with `{{= for =}}`"
            ));
        }
        Datum::Record(_) => {
            return Err(format!(
                "`{path}` (line {line}) is a record: name one of its parts"
            ));
        }
    };
    filters
        .iter()
        .try_fold(text, |text, filter| apply_filter(filter, &text))
        .map_err(|why| format!("`{path}` (line {line}): {why}"))
}

fn loop_items(
    item: &str,
    list: &str,
    line: usize,
    lookup: &Lookup<'_>,
    shown: &str,
) -> Result<Vec<Datum>, String> {
    let tag = format!("`{{= for {item} in {list} =}}` (line {line})");
    match lookup.named(list) {
        Some(Datum::List(items)) => Ok(items.clone()),
        Some(_) => Err(format!("{tag}: `{list}` is not a list")),
        None => Err(format!(
            "{tag}: `{list}` is not a name a template may use (in `{shown}`)"
        )),
    }
}

fn taken_branch<'b, 't>(
    branches: &'b [(Condition<'t>, Vec<Node<'t>>)],
    lookup: &Lookup<'_>,
    shown: &str,
) -> Result<Option<&'b Vec<Node<'t>>>, String> {
    for (condition, body) in branches {
        if decide(condition, lookup, shown)? {
            return Ok(Some(body));
        }
    }
    Ok(None)
}

fn walk_in_scope<'a, 't>(
    nodes: &'a [Node<'t>],
    scope: &Scope<'a>,
    visit: &mut dyn FnMut(&'a Path<'t>, Scope<'a>),
) {
    for node in nodes {
        match node {
            Node::Value { path, .. } => visit_path(path, scope, visit),
            Node::For {
                item, list, body, ..
            } => {
                let mut inner = scope.clone();
                inner.loops.push((item, list));
                walk_in_scope(body, &inner, visit);
            }
            Node::If {
                branches,
                otherwise,
                ..
            } => {
                for (condition, body) in branches {
                    paths_of(condition)
                        .into_iter()
                        .for_each(|path| visit_path(path, scope, visit));
                    walk_in_scope(body, scope, visit);
                }
                walk_in_scope(otherwise, scope, visit);
            }
            Node::Text(_) | Node::Hole { .. } => {}
        }
    }
}

/// Visits a path, then each path inside its brackets.
fn visit_path<'a, 't>(
    path: &'a Path<'t>,
    scope: &Scope<'a>,
    visit: &mut dyn FnMut(&'a Path<'t>, Scope<'a>),
) {
    visit(path, scope.clone());
    for step in &path.steps {
        if let Step::Key(key) = step {
            visit_path(key, scope, visit);
        }
    }
}

fn all_holes<'t>(nodes: &[Node<'t>]) -> Vec<(&'t str, usize)> {
    holes_in(nodes)
        .into_iter()
        .filter_map(|node| match node {
            Node::Hole { name, line, .. } => Some((*name, *line)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Datum {
        Datum::Text(value.to_string())
    }

    fn record(entries: &[(&str, Datum)]) -> Datum {
        Datum::Record(
            entries
                .iter()
                .map(|(name, datum)| ((*name).to_string(), datum.clone()))
                .collect(),
        )
    }

    fn list_of(items: &[(&str, &str, bool)]) -> Datum {
        Datum::List(
            items
                .iter()
                .map(|(name, kind, optional)| {
                    record(&[
                        ("name", text(name)),
                        ("kind", text(kind)),
                        ("optional", Datum::Flag(*optional)),
                    ])
                })
                .collect(),
        )
    }

    fn bindings() -> Bindings {
        Bindings::from_pairs(&[("model", "dish"), ("description", "")])
            .with(
                "fields",
                list_of(&[
                    ("name", "text", false),
                    ("price", "int", false),
                    ("notes", "text", true),
                ]),
            )
            .with(
                "kinds",
                record(&[
                    ("text", record(&[("rust", text("String"))])),
                    ("int", record(&[("rust", text("i64"))])),
                ]),
            )
    }

    fn rendered_with(template: &str, bindings: &Bindings) -> Result<String, String> {
        let nodes = parse_body(template)?;
        expand(&nodes, bindings, template)?
            .into_iter()
            .map(|part| match part {
                Expanded::Text(text) => Ok(text),
                Expanded::Hole { name, .. } => Ok(format!("<{name}>")),
            })
            .collect()
    }

    fn rendered(template: &str) -> Result<String, String> {
        rendered_with(template, &bindings())
    }

    #[test]
    fn a_for_loops_over_each_item() {
        assert_eq!(
            rendered("{= for f in fields =}{= f.name =} {= end =}"),
            Ok("name price notes ".into())
        );
    }

    #[test]
    fn control_only_lines_vanish() {
        let template = "struct X {\n    {= for f in fields =}\n    {= f.name =}: {= kinds[f.kind].rust =},\n    {= end =}\n}\n";
        assert_eq!(
            rendered(template),
            Ok("struct X {\n    name: String,\n    price: i64,\n    notes: String,\n}\n".into())
        );
    }

    #[test]
    fn a_control_tag_on_the_first_or_last_line_vanishes_too() {
        assert_eq!(
            rendered("{= if model =}\nyes\n{= end =}"),
            Ok("yes\n".into())
        );
        assert_eq!(
            rendered("a\n  {= if model =}  \nb\n{= end =}\n"),
            Ok("a\nb\n".into())
        );
    }

    #[test]
    fn loop_last_drops_the_last_comma() {
        assert_eq!(
            rendered("{= for f in fields =}{= f.name =}{= if not loop.last =}, {= end =}{= end =}"),
            Ok("name, price, notes".into())
        );
        assert_eq!(
            rendered("{= for f in fields =}{= loop.index =} {= end =}"),
            Ok("1 2 3 ".into())
        );
        assert_eq!(
            rendered("{= for f in fields =}{= if loop.first =}{= f.name =}{= end =}{= end =}"),
            Ok("name".into())
        );
    }

    #[test]
    fn if_elif_else_picks_one_branch() {
        let template =
            "{= if model == \"dish\" =}a{= elif model == \"menu\" =}b{= else =}c{= end =}";
        let with_model = |model: &str| bindings().with("model", text(model));
        assert_eq!(rendered(template), Ok("a".into()));
        assert_eq!(rendered_with(template, &with_model("menu")), Ok("b".into()));
        assert_eq!(rendered_with(template, &with_model("x")), Ok("c".into()));
    }

    #[test]
    fn and_or_not_follow_precedence() {
        // ((not a) and b) or c with a, b, c all true is false or true = true; read as
        // not (a and (b or c)) it would be false.
        let flags = Bindings::default()
            .with("a", Datum::Flag(true))
            .with("b", Datum::Flag(true))
            .with("c", Datum::Flag(true));
        assert_eq!(
            rendered_with("{= if not a and b or c =}yes{= end =}", &flags),
            Ok("yes".into())
        );
        // and before or: a or b and c with a true, b false is true; (a or b) and c with c
        // false would be false.
        let mixed = flags
            .with("b", Datum::Flag(false))
            .with("c", Datum::Flag(false));
        assert_eq!(
            rendered_with("{= if a or b and c =}yes{= end =}", &mixed),
            Ok("yes".into())
        );
    }

    #[test]
    fn an_optional_item_sets_its_flag() {
        assert_eq!(
            rendered("{= for f in fields =}{= if f.optional =}?{= end =}{= end =}"),
            Ok("?".into())
        );
    }

    #[test]
    fn text_truth_is_not_empty() {
        assert_eq!(
            rendered("{= if description =}yes{= end =}"),
            Ok(String::new())
        );
        assert_eq!(rendered("{= if model =}yes{= end =}"), Ok("yes".into()));
    }

    #[test]
    fn an_empty_list_loops_zero_times() {
        let empty = bindings().with("fields", Datum::List(Vec::new()));
        assert_eq!(
            rendered_with(
                "a{= for f in fields =}x{= end =}{= if fields =}y{= end =}b",
                &empty
            ),
            Ok("ab".into())
        );
    }

    #[test]
    fn filters_apply_to_paths() {
        assert_eq!(
            rendered(
                "{= for f in fields =}{= if loop.first =}{= f.name | pascal =}{= end =}{= end =}"
            ),
            Ok("Name".into())
        );
    }

    #[test]
    fn a_hole_inside_if_is_expanded_as_a_hole() {
        let template = "a{= if model =}{= ? check: block | do it =}{= end =}b";
        let nodes = parse_body(template).unwrap();
        assert_eq!(
            expand(&nodes, &bindings(), template),
            Ok(vec![
                Expanded::Text("a".into()),
                Expanded::Hole {
                    name: "check",
                    kind: "block",
                    instruction: "do it"
                },
                Expanded::Text("b".into()),
            ])
        );
    }

    #[test]
    fn structure_mistakes_name_the_line() {
        assert_eq!(
            parse_body("{= for f in fields =}x").unwrap_err(),
            "`{= for f in fields =}` (line 1) has no `{= end =}`"
        );
        assert_eq!(
            parse_body("x\n{= end =}").unwrap_err(),
            "`{= end =}` (line 2) closes nothing"
        );
        assert_eq!(
            parse_body("{= else =}").unwrap_err(),
            "`{= else =}` (line 1) is outside an `{= if =}`"
        );
        assert_eq!(
            parse_body("{= if (a) =}").unwrap_err(),
            "`{= if (a) =}` (line 1): parentheses are not supported; split the condition with elif"
        );
    }

    #[test]
    fn expansion_mistakes_name_the_line() {
        assert_eq!(
            rendered("{= for f in model =}{= end =}").unwrap_err(),
            "`{= for f in model =}` (line 1): `model` is not a list"
        );
        assert_eq!(
            rendered("{= for f in fields =}{= f.colour =}{= end =}").unwrap_err(),
            "`f.colour` (line 1): an item has no `colour` (it has kind, name, optional)"
        );
        assert_eq!(
            rendered("{= for f in fields =}{= if loop.last == \"x\" =}{= end =}{= end =}")
                .unwrap_err(),
            "`loop.last` (line 1) is a flag: test it with `if` or `not`"
        );
    }

    #[test]
    fn the_walks_see_paths_loops_and_holes() {
        let template = "{= for f in fields =}\n{= kinds[f.kind].rust =}\n{= ? check: block =}\n{= end =}\n{= if model =}{= end =}";
        let nodes = parse_body(template).unwrap();
        let mut seen = Vec::new();
        walk_paths(&nodes, &mut |path, scope| {
            seen.push((path.to_string(), scope.loops.len()))
        });
        assert_eq!(
            seen,
            [
                ("kinds[f.kind].rust".to_string(), 1),
                ("f.kind".to_string(), 1),
                ("model".to_string(), 0)
            ]
        );
        assert_eq!(holes_in_loops(&nodes), [("check", 3)]);
        assert_eq!(loops_in(&nodes), [("f", "fields", 1)]);
    }
}
