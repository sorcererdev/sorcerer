//! Facts: what the code can tell about a unit without a model. Decided by
//! node kinds tree-sitter grammars share, never by a language's name.

use tree_sitter::{Node, Parser, Tree};

use crate::domain::language::Loaded;
use crate::domain::unit::{Span, Unit, lines_of};
use crate::{Error, Result};

/// What the code alone says about one unit, without asking a model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// Lines (counted from 1) where the parser found an error or a missing token.
    pub parse_errors: Vec<u32>,
    /// Lines of comments that say `TODO` or `FIXME`.
    pub todo_lines: Vec<u32>,
    /// True when the unit has no body, or its body holds only placeholders (`pass`, `...`,
    /// `todo!()`) and comments.
    pub body_empty: bool,
}

/// Node kinds that do nothing, as tree-sitter grammars commonly name them:
/// a `pass`, and a statement wrapping only an ellipsis.
const EMPTY_BODY_KINDS: &[&str] = &["pass_statement", "ellipsis"];
/// Macros that stand for a body not written yet.
const PLACEHOLDER_MACROS: &[&str] = &["todo", "unimplemented"];

/// Reads the facts of one unit of a file: its parse errors, its TODO and FIXME comments, and
/// whether its body is still empty.
///
/// The file is parsed whole (a unit's text alone may not parse), then only the syntax nodes that
/// share a line with the unit are looked at. When the parser gives up, the facts are empty.
///
/// Example: for `def f():\n    # TODO: retry\n    pass\n`, the unit `f` has no parse errors,
/// a TODO on line 2, and an empty body.
pub fn facts_of(lang: &Loaded, text: &str, unit: &Unit) -> Result<Facts> {
    let Some(tree) = parse(lang, text)? else {
        return Ok(Facts::default());
    };
    let nodes = nodes_sharing_lines_with(&tree, unit.span);
    Ok(Facts {
        parse_errors: parse_error_lines(&nodes, unit.span),
        todo_lines: todo_comment_lines(&nodes, unit.span, text),
        body_empty: body_is_empty(&nodes, unit, text),
    })
}

/// The syntax tree of the whole file; `None` when tree-sitter gives up on it.
fn parse(lang: &Loaded, text: &str) -> Result<Option<Tree>> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.grammar)
        .map_err(|e| Error::GrammarLoad {
            name: lang.language.name.clone(),
            why: e.to_string(),
        })?;
    Ok(parser.parse(text, None))
}

/// A syntax node with the name of the field it fills in its parent (`body`, `name`, …), if any.
type NodeInField<'t> = (Node<'t>, Option<&'static str>);

/// Every node that shares at least one line with the span, in reading order (a parent before
/// its children). A node wholly outside the span is skipped with everything inside it.
fn nodes_sharing_lines_with(tree: &Tree, span: Span) -> Vec<NodeInField<'_>> {
    let mut cursor = tree.walk();
    let mut found = Vec::new();
    loop {
        let node = cursor.node();
        let shares_lines = overlaps(span, lines_of(&node));
        if shares_lines {
            found.push((node, cursor.field_name()));
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
fn parse_error_lines(nodes: &[NodeInField], span: Span) -> Vec<u32> {
    let mut lines: Vec<u32> = nodes
        .iter()
        .filter(|(node, _)| node.is_error() || node.is_missing())
        .map(|(node, _)| first_line(node))
        .filter(|line| span.holds(*line))
        .collect();
    lines.dedup();
    lines
}

/// The lines inside the span of comments that say `TODO` or `FIXME`, each line once. A comment
/// over several lines is looked at line by line.
fn todo_comment_lines(nodes: &[NodeInField], span: Span, text: &str) -> Vec<u32> {
    let mut lines: Vec<u32> = nodes
        .iter()
        .filter(|(node, _)| node.kind().contains("comment"))
        .flat_map(|(node, _)| {
            let comment = node.utf8_text(text.as_bytes()).unwrap_or_default();
            (first_line(node)..)
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

/// Whether the unit's body is missing or does nothing yet. The body node is the first node in
/// a `body` field whose lines are exactly the unit's body lines.
fn body_is_empty(nodes: &[NodeInField], unit: &Unit, text: &str) -> bool {
    let body = nodes.iter().find(|(node, field)| {
        let lines = lines_of(node);
        *field == Some("body") && Some(lines) == unit.body && unit.span.contains(&lines)
    });
    match body {
        Some((node, _)) => is_placeholder_body(node, text.as_bytes()),
        None => unit.body.is_none(),
    }
}
/// True when every statement in the body, comments aside, is a placeholder. An empty body
/// (`{}`) counts.
fn is_placeholder_body(body: &Node, src: &[u8]) -> bool {
    let mut c = body.walk();
    body.named_children(&mut c)
        .filter(|n| !n.kind().contains("comment"))
        .all(|n| is_placeholder(&n, src))
}

/// True for a statement that does nothing yet: `pass`, `...`, `todo!()`, `unimplemented!()`, or a
/// statement wrapping exactly one of those.
fn is_placeholder(n: &Node, src: &[u8]) -> bool {
    if EMPTY_BODY_KINDS.contains(&n.kind()) {
        return true;
    }
    if n.kind() == "macro_invocation" {
        let name = n
            .child_by_field_name("macro")
            .and_then(|m| m.utf8_text(src).ok())
            .unwrap_or_default();
        return PLACEHOLDER_MACROS.contains(&name);
    }
    // a statement wrapping one placeholder (`...`, `todo!();`)
    let mut c = n.walk();
    let kids: Vec<Node> = n.named_children(&mut c).collect();
    n.kind().ends_with("statement") && kids.len() == 1 && is_placeholder(&kids[0], src)
}
