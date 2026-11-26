//! Facts: what the code can tell about a unit without a model. Decided by what the language's
//! queries capture (`@comment.outer`, `@placeholder`, a definition's `@function.inner`), never by
//! a language's name or a grammar's node kinds.

use tree_sitter::{Node, Tree};

use crate::Result;
use crate::domain::captures::{COMMENT, Captures, PLACEHOLDER};
use crate::domain::language::Loaded;
use crate::domain::unit::{Span, Unit, is_unit_capture, lines_of};

/// What the code alone says about one unit, without asking a model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// Lines (counted from 1) where the parser found an error or a missing token.
    pub parse_errors: Vec<u32>,
    /// Lines of comments that say `TODO` or `FIXME`.
    pub todo_lines: Vec<u32>,
    /// True when the unit has no body, or its body holds only comments and what the language's
    /// `placeholders.scm` captures as `@placeholder` (`pass`, `...`, `todo!()`).
    pub body_empty: bool,
}

/// Reads the facts of one unit of a file: its parse errors, its TODO and FIXME comments, and
/// whether its body is still empty.
///
/// The file is parsed whole (a unit's text alone may not parse), then only the syntax nodes that
/// share a line with the unit are looked at. When the parser gives up, the facts are empty.
///
/// Example: for `def f():\n    # TODO: retry\n    pass\n`, the unit `f` has no parse errors,
/// a TODO on line 2, and an empty body.
pub fn facts_of(lang: &Loaded, text: &str, unit: &Unit) -> Result<Facts> {
    let Some(tree) = lang.parse(text)? else {
        return Ok(Facts::default());
    };
    let nodes = nodes_sharing_lines_with(&tree, unit.span);
    let captures = Captures::of(lang, &tree, text);
    Ok(Facts {
        parse_errors: parse_error_lines(&nodes, unit.span),
        todo_lines: todo_comment_lines(&captures, unit.span, text),
        body_empty: body_is_empty(&captures, unit),
    })
}

/// The lines (counted from 1) where the parser found an error or a missing token anywhere in a
/// file, each line once, in order. When the parser gives up, the list is empty.
///
/// Example: for `def f(x):\n    return x +\n\ndef g():\n    return 1\n`, the list is `[2]`;
/// a file that parses gives `[]`.
pub fn parse_errors_of(lang: &Loaded, text: &str) -> Result<Vec<u32>> {
    let Some(tree) = lang.parse(text)? else {
        return Ok(Vec::new());
    };
    let whole_file = Span {
        start: 1,
        end: u32::MAX,
    };
    let mut lines = parse_error_lines(&nodes_sharing_lines_with(&tree, whole_file), whole_file);
    lines.sort_unstable();
    lines.dedup();
    Ok(lines)
}

/// Every node that shares at least one line with the span, in reading order (a parent before
/// its children). A node wholly outside the span is skipped with everything inside it.
fn nodes_sharing_lines_with(tree: &Tree, span: Span) -> Vec<Node<'_>> {
    let mut cursor = tree.walk();
    let mut found = Vec::new();
    loop {
        let node = cursor.node();
        let shares_lines = overlaps(span, lines_of(&node));
        if shares_lines {
            found.push(node);
        }
        if shares_lines && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return found;
            }
        }
    }
}

fn overlaps(a: Span, b: Span) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// The lines inside the span where the parser marked an error or inserted a missing token,
/// each line once.
fn parse_error_lines(nodes: &[Node], span: Span) -> Vec<u32> {
    let mut lines: Vec<u32> = nodes
        .iter()
        .filter(|node| node.is_error() || node.is_missing())
        .map(first_line)
        .filter(|line| span.holds(*line))
        .collect();
    lines.dedup();
    lines
}

/// The lines inside the span of comments that say `TODO` or `FIXME`, each line once. A comment
/// over several lines is looked at line by line.
fn todo_comment_lines(captures: &Captures, span: Span, text: &str) -> Vec<u32> {
    let mut lines: Vec<u32> = captures
        .nodes(&[COMMENT])
        .into_iter()
        .flat_map(|node| {
            let comment = node.utf8_text(text.as_bytes()).unwrap_or_default();
            (first_line(&node)..)
                .zip(comment.lines())
                .filter(|(_, line)| line.contains("TODO") || line.contains("FIXME"))
                .map(|(number, _)| number)
                .collect::<Vec<u32>>()
        })
        .filter(|line| span.holds(*line))
        .collect();
    lines.dedup();
    lines
}

fn first_line(node: &Node) -> u32 {
    node.start_position().row as u32 + 1
}

/// Whether the unit's body is missing or does nothing yet: every item of it, comments aside,
/// is a `@placeholder`. The definition is the one the queries captured inside the unit's lines
/// whose body has exactly the unit's body lines.
fn body_is_empty(captures: &Captures, unit: &Unit) -> bool {
    let Some(body) = unit.body else {
        return true;
    };
    let definition = unit_definitions(captures).into_iter().find(|node| {
        unit.span.contains(&lines_of(node)) && body_lines(captures, *node) == Some(body)
    });
    definition.is_some_and(|node| {
        captures
            .body_items(node)
            .iter()
            .all(|item| captures.is(PLACEHOLDER, *item))
    })
}

/// Every node captured as a unit (`@definition.<kind>`, `@unit.<…>`).
fn unit_definitions<'t>(captures: &Captures<'t>) -> Vec<Node<'t>> {
    let mut nodes: Vec<Node<'t>> = captures
        .matches()
        .iter()
        .flatten()
        .filter(|(capture, _)| is_unit_capture(capture))
        .map(|(_, node)| *node)
        .collect();
    nodes.sort_by_key(|node| (node.start_byte(), node.id()));
    nodes.dedup_by_key(|node| node.id());
    nodes
}

/// The lines of what the queries capture as the definition's body.
fn body_lines(captures: &Captures, definition: Node) -> Option<Span> {
    let body = captures.body_of(definition);
    let start = body.iter().map(|node| lines_of(node).start).min()?;
    let end = body.iter().map(|node| lines_of(node).end).max()?;
    Some(Span { start, end })
}
