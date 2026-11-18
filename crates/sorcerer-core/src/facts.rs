//! Facts: what the code can tell about a unit without a model. Decided by
//! node kinds tree-sitter grammars share, never by a language's name.

use tree_sitter::{Node, Parser, TreeCursor};

use crate::language::Loaded;
use crate::unit::{Span, Unit, lines_of};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// Lines (1-based) where the parser found an error or a missing token.
    pub parse_errors: Vec<u32>,
    /// Lines of comments saying `TODO` or `FIXME`.
    pub todo_lines: Vec<u32>,
    /// The body is absent, or holds only placeholders and comments.
    pub body_empty: bool,
}

/// Node kinds that do nothing, as tree-sitter grammars commonly name them:
/// a `pass`, and a statement wrapping only an ellipsis.
const EMPTY_BODY_KINDS: &[&str] = &["pass_statement", "ellipsis"];
/// Macros that stand for a body not written yet.
const PLACEHOLDER_MACROS: &[&str] = &["todo", "unimplemented"];

pub fn facts_of(lang: &Loaded, text: &str, unit: &Unit) -> Result<Facts> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.grammar)
        .map_err(|e| Error::GrammarLoad {
            name: lang.language.name.clone(),
            why: e.to_string(),
        })?;
    let Some(tree) = parser.parse(text, None) else {
        return Ok(Facts::default());
    };
    let src = text.as_bytes();
    let mut facts = Facts {
        body_empty: unit.body.is_none(),
        ..Facts::default()
    };
    let mut body: Option<Node> = None;
    walk(&mut tree.walk(), &mut |node, field| {
        let lines = lines_of(&node);
        if !overlaps(unit.span, lines) {
            return false;
        }
        if node.is_error() || node.is_missing() {
            let line = node.start_position().row as u32 + 1;
            if unit.span.holds(line) {
                facts.parse_errors.push(line);
            }
        }
        if node.kind().contains("comment") {
            let first = node.start_position().row as u32 + 1;
            let comment = node.utf8_text(src).unwrap_or_default();
            for (i, l) in comment.lines().enumerate() {
                if (l.contains("TODO") || l.contains("FIXME")) && unit.span.holds(first + i as u32)
                {
                    facts.todo_lines.push(first + i as u32);
                }
            }
        }
        if body.is_none()
            && field == Some("body")
            && Some(lines) == unit.body
            && unit.span.contains(&lines)
        {
            body = Some(node);
        }
        true
    });
    facts.parse_errors.dedup();
    facts.todo_lines.dedup();
    if let Some(b) = body {
        facts.body_empty = is_placeholder_body(&b, src);
    }
    Ok(facts)
}

fn overlaps(a: Span, b: Span) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// Preorder over the tree; F says whether to enter a node's children.
fn walk<'t>(c: &mut TreeCursor<'t>, f: &mut impl FnMut(Node<'t>, Option<&str>) -> bool) {
    loop {
        if f(c.node(), c.field_name()) && c.goto_first_child() {
            continue;
        }
        while !c.goto_next_sibling() {
            if !c.goto_parent() {
                return;
            }
        }
    }
}

fn is_placeholder_body(body: &Node, src: &[u8]) -> bool {
    let mut c = body.walk();
    body.named_children(&mut c)
        .filter(|n| !n.kind().contains("comment"))
        .all(|n| is_placeholder(&n, src))
}

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
