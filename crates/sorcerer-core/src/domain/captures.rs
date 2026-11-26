//! What a language's query files capture in one file: the only way the engine looks at a
//! grammar's tree. A grammar's node kinds and field names stay in the queries; the code reads
//! the capture names below, which every language that wants the feature writes.
//!
//! | capture | written in | what it marks |
//! |---|---|---|
//! | `@definition.<kind>`, `@name` | `tags.scm` (and `impls.scm`) | a definition and its name |
//! | `@reference.call`, `@name` | `tags.scm` (and `calls.scm`) | a call (or a struct literal) and the name it calls |
//! | `@function.outer`, `@class.outer` | `textobjects.scm` | a whole definition, decorators or attributes included |
//! | `@function.inner`, `@class.inner` | `textobjects.scm` | a definition's body: the block, or the items inside it |
//! | `@statement.outer` | `textobjects.scm` | one statement of a block |
//! | `@parameter.inner` | `textobjects.scm` | one item of a bracketed list |
//! | `@comment.outer` | `textobjects.scm` | a comment |
//! | `@import.path` | `imports.scm` | what an import names |
//! | `@placeholder` | `placeholders.scm` | a statement that does nothing yet (`pass`, `todo!()`) |
//! | `@docstring` | `docstrings.scm` | a string that documents the file |
//! | `@parameters`, `@arguments` | `lists.scm` | a definition's parameter list, a call's argument list |
//!
//! A language without one of these files simply has no such capture: the feature that reads it
//! finds nothing (no comments, no placeholders), it does not guess.

use tree_sitter::{Node, QueryCursor, StreamingIterator, Tree};

use crate::domain::language::Loaded;

/// A comment, as `textobjects.scm` captures it.
pub const COMMENT: &str = "comment.outer";
/// A statement that does nothing yet, as `placeholders.scm` captures it.
pub const PLACEHOLDER: &str = "placeholder";
/// A string documenting the file, as `docstrings.scm` captures it.
pub const DOCSTRING: &str = "docstring";
/// A definition's parameter list, as `lists.scm` captures it.
pub const PARAMETERS: &str = "parameters";
/// A call's argument list, as `lists.scm` captures it.
pub const ARGUMENTS: &str = "arguments";
/// One item of a bracketed list, as `textobjects.scm` captures it.
pub const LIST_ITEM: &str = "parameter.inner";

/// The captures that hold a whole definition.
const OUTER: [&str; 2] = ["function.outer", "class.outer"];
/// The captures that hold a definition's body.
const INNER: [&str; 2] = ["function.inner", "class.inner"];
/// One statement of a block.
const STATEMENT: &str = "statement.outer";

/// One match of one query: each capture's name and node, in the match's order.
pub type CapturedMatch<'t> = Vec<(String, Node<'t>)>;

/// Every match of every query file of a language over one file's tree.
pub struct Captures<'t> {
    matches: Vec<CapturedMatch<'t>>,
}

impl<'t> Captures<'t> {
    /// Runs each of the language's query files over the tree and keeps what they capture.
    pub fn of(lang: &Loaded, tree: &'t Tree, text: &str) -> Captures<'t> {
        let mut matches = Vec::new();
        for (_, query) in &lang.queries {
            let names = query.capture_names();
            let mut cursor = QueryCursor::new();
            let mut found = cursor.matches(query, tree.root_node(), text.as_bytes());
            while let Some(hit) = found.next() {
                matches.push(
                    hit.captures
                        .iter()
                        .map(|c| (names[c.index as usize].to_string(), c.node))
                        .collect(),
                );
            }
        }
        Captures { matches }
    }

    /// Every match, for a reader that needs the captures of one match together (a definition
    /// and its name).
    pub fn matches(&self) -> &[CapturedMatch<'t>] {
        &self.matches
    }

    /// The nodes captured under any of the names, in file order, each node once.
    ///
    /// Example: `nodes(&["comment.outer"])` gives a file's comments, first to last.
    pub fn nodes(&self, names: &[&str]) -> Vec<Node<'t>> {
        let mut found: Vec<Node<'t>> = self
            .matches
            .iter()
            .flatten()
            .filter(|(name, _)| names.contains(&name.as_str()))
            .map(|(_, node)| *node)
            .collect();
        in_file_order(&mut found);
        found
    }

    /// Whether the node itself is captured under the name.
    pub fn is(&self, name: &str, node: Node) -> bool {
        self.matches
            .iter()
            .flatten()
            .any(|(captured, other)| captured == name && other.id() == node.id())
    }

    /// The nodes captured under `capture`, each with the text of its match's `@name`, in file
    /// order. A node two patterns capture is kept once.
    ///
    /// Example: over `tags.scm`, `named("definition.class", text)` gives each class with its
    /// name, `(node of class Order, "Order")`.
    pub fn named(&self, capture: &str, text: &str) -> Vec<(Node<'t>, String)> {
        let mut found: Vec<(Node<'t>, String)> = self
            .matches
            .iter()
            .filter_map(|captures| {
                let node = captures.iter().find(|(name, _)| name == capture)?.1;
                let name = captures.iter().find(|(name, _)| name == "name")?.1;
                Some((node, text[name.byte_range()].to_string()))
            })
            .collect();
        found.sort_by_key(|(node, _)| (node.start_byte(), node.id()));
        found.dedup_by_key(|(node, _)| node.id());
        found
    }

    /// The items of a definition's body, in order, comments left out: what `textobjects.scm`
    /// captures as its `@function.inner` or `@class.inner`, and, when that capture is a block
    /// whose children are `@statement.outer`s, those statements.
    ///
    /// Example: for `def f():\n    x = 1\n    # done\n` the items are `x = 1`; for Rust's
    /// `fn f() { todo!() }`, the `todo!()`. A body with nothing in it (`fn f() {}`) has none.
    pub fn body_items(&self, definition: Node<'t>) -> Vec<Node<'t>> {
        self.body_of(definition)
            .into_iter()
            .flat_map(|inner| self.statements_of(inner).unwrap_or_else(|| vec![inner]))
            .filter(|item| item.is_named() && !self.is(COMMENT, *item))
            .collect()
    }

    /// What `textobjects.scm` captures as the definition's own body, comments included: the
    /// `@function.inner`/`@class.inner` captures inside it that no smaller definition holds.
    pub fn body_of(&self, definition: Node<'t>) -> Vec<Node<'t>> {
        let outers = self.nodes(&OUTER);
        self.nodes(&INNER)
            .into_iter()
            .filter(|inner| inner.is_named())
            .filter(|inner| is_own(definition, *inner, &outers))
            .collect()
    }

    /// The capture under `name` that belongs to the definition itself (a nested definition's
    /// does not): its parameter list, say.
    pub fn own(&self, name: &str, definition: Node<'t>) -> Option<Node<'t>> {
        let outers = self.nodes(&OUTER);
        self.nodes(&[name])
            .into_iter()
            .find(|node| is_own(definition, *node, &outers))
    }

    /// The items of a bracketed list, in order, comments left out: the `@parameter.inner`
    /// captures whose parent is the list.
    pub fn list_items(&self, list: Node<'t>) -> Vec<Node<'t>> {
        self.nodes(&[LIST_ITEM])
            .into_iter()
            .filter(|item| item.parent().is_some_and(|parent| parent.id() == list.id()))
            .filter(|item| !self.is(COMMENT, *item))
            .collect()
    }

    /// A block's statements, when its children are captured as `@statement.outer`.
    fn statements_of(&self, block: Node<'t>) -> Option<Vec<Node<'t>>> {
        let mut cursor = block.walk();
        let children: Vec<Node<'t>> = block.named_children(&mut cursor).collect();
        let statements: Vec<Node<'t>> = children
            .into_iter()
            .filter(|child| self.is(STATEMENT, *child))
            .collect();
        (!statements.is_empty()).then_some(statements)
    }
}

/// Whether the node is inside the definition and no smaller whole definition (an
/// `@function.outer`/`@class.outer` strictly inside it) holds it.
fn is_own(definition: Node, node: Node, outers: &[Node]) -> bool {
    let strictly_holds =
        |outer: &Node| holds(*outer, node) && outer.byte_range() != node.byte_range();
    strictly_holds(&definition)
        && !outers.iter().any(|outer| {
            strictly_holds(outer)
                && holds(definition, *outer)
                && outer.byte_range() != definition.byte_range()
        })
}

fn holds(outer: Node, inner: Node) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

fn in_file_order(nodes: &mut Vec<Node>) {
    nodes.sort_by_key(|node| {
        (
            node.start_byte(),
            std::cmp::Reverse(node.end_byte()),
            node.id(),
        )
    });
    nodes.dedup_by_key(|node| node.id());
}
