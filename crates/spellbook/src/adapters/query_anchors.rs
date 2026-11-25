//! Anchor phrases resolved with the language's own queries: `imports.scm` for the imports,
//! `tags.scm` for the definitions and calls by name, `textobjects.scm` for a body's statements.
//! A language gets anchors by having those three queries; nothing here names one.

use std::path::Path;

use sorcerer_core::Result;
use sorcerer_core::domain::language::Loaded;
use tree_sitter::{Node, QueryCursor, StreamingIterator, Tree};

use crate::domain::anchor::{
    Anchor, Place, Position, Resolution, Target, indent_of, resolve_in_text,
};
use crate::ports::anchors::Anchors;
use crate::ports::grammars::Grammars;

/// The anchors over a set of grammars: the wiring's own, or a borrowed one in a test.
pub struct QueryAnchors<G> {
    grammars: G,
}

impl<G: Grammars> QueryAnchors<G> {
    pub fn new(grammars: G) -> QueryAnchors<G> {
        QueryAnchors { grammars }
    }
}

impl<G: Grammars> Anchors for QueryAnchors<G> {
    fn resolve(&self, file: &Path, text: &str, anchor: &Anchor) -> Result<Resolution> {
        if let Some(found) = resolve_in_text(anchor, text) {
            return Ok(found);
        }
        let Some(loaded) = self.grammars.for_path(file)? else {
            return Ok(refused(
                "no language covers the file, so only `marker`, `line matching` and `file` work in it",
            ));
        };
        let Some(tree) = loaded.parse(text)? else {
            return Ok(refused("the file cannot be parsed"));
        };
        let parsed = Parsed {
            loaded: &loaded,
            tree: &tree,
            text,
        };
        Ok(match &anchor.target {
            Target::Imports => parsed.imports(anchor.position),
            Target::Function(name) => {
                parsed.in_definition(Definition::Function, name, anchor.position)
            }
            Target::Class(name) => parsed.in_definition(Definition::Class, name, anchor.position),
            Target::ParametersIn(name) => parsed.parameters(name, anchor.position),
            Target::Call { name, within } => parsed.call(name, within.as_deref(), anchor.position),
            _ => refused("this target needs no grammar"),
        })
    }
}

fn refused(why: &str) -> Resolution {
    Resolution::Refused { why: why.into() }
}

/// What `tags.scm` tags a definition as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Definition {
    Function,
    Class,
}

impl Definition {
    fn word(self) -> &'static str {
        match self {
            Definition::Function => "function",
            Definition::Class => "class",
        }
    }
}

/// A file parsed in its language.
struct Parsed<'a> {
    loaded: &'a Loaded,
    tree: &'a Tree,
    text: &'a str,
}

impl<'a> Parsed<'a> {
    /// `end of imports`: after the last top-level import; `start of`: before the first. A file
    /// with none takes them after its leading comments and docstring.
    fn imports(&self, position: Position) -> Resolution {
        let imports = self.top_level_imports();
        let row = match (imports.first(), imports.last(), position) {
            (Some(first), _, Position::Start | Position::Before) => first.start_position().row,
            (_, Some(last), _) => last.end_position().row + 1,
            _ => self.row_after_the_preamble(),
        };
        lines_at(row, String::new(), String::new())
    }

    /// The top-level statements holding an `@import.path`, in file order. A statement with a
    /// body (a function, a class, a module block) holds its imports inside, so it is not one.
    fn top_level_imports(&self) -> Vec<Node<'a>> {
        let mut statements: Vec<Node<'a>> = self
            .captures("imports.scm", "import.path")
            .into_iter()
            .filter_map(|capture| self.top_level_ancestor(capture))
            .filter(|statement| statement.child_by_field_name("body").is_none())
            .collect();
        statements.sort_by_key(Node::start_byte);
        statements.dedup_by_key(|statement| statement.id());
        statements
    }

    /// The row after the file's leading comments and its docstring (a first statement that is
    /// only a string), or 0.
    fn row_after_the_preamble(&self) -> usize {
        let root = self.tree.root_node();
        let mut cursor = root.walk();
        root.named_children(&mut cursor)
            .take_while(|node| is_comment(*node) || is_only_a_string(*node))
            .last()
            .map_or(0, |node| node.end_position().row + 1)
    }

    /// Inside a function's or class's body (`start of`/`end of`, indented like its statements),
    /// or around the definition (`before`/`after`, indented like it).
    fn in_definition(&self, kind: Definition, name: &str, position: Position) -> Resolution {
        let definition = match self.the_one(kind, name) {
            Ok(definition) => definition,
            Err(miss) => return miss,
        };
        let indent_at = |row: usize| indent_of(self.line(row));
        match position {
            Position::Before => {
                let row = definition.start_position().row;
                lines_at(row, indent_at(row), String::new())
            }
            Position::After => lines_at(
                definition.end_position().row + 1,
                indent_at(definition.start_position().row),
                String::new(),
            ),
            Position::Start | Position::End => {
                let items = self.body_items(kind, definition);
                let item = if position == Position::Start {
                    items.first()
                } else {
                    items.last()
                };
                match item {
                    None => refused(&format!("{} `{name}` has an empty body", kind.word())),
                    Some(item) if position == Position::Start => {
                        let row = item.start_position().row;
                        lines_at(row, indent_at(row), String::new())
                    }
                    Some(item) => lines_at(
                        item.end_position().row + 1,
                        indent_at(item.start_position().row),
                        String::new(),
                    ),
                }
            }
        }
    }

    /// Inside the function's parameter list.
    fn parameters(&self, name: &str, position: Position) -> Resolution {
        let function = match self.the_one(Definition::Function, name) {
            Ok(function) => function,
            Err(miss) => return miss,
        };
        match function.child_by_field_name("parameters") {
            Some(list) => self.in_list(list, position),
            None => refused(&format!("function `{name}` has no parameter list")),
        }
    }

    /// Inside the argument list of the one call named `name` (inside `within` when given).
    fn call(&self, name: &str, within: Option<&str>, position: Position) -> Resolution {
        let around = match within {
            None => None,
            Some(function) => match self.the_one(Definition::Function, function) {
                Ok(found) => Some(found),
                Err(miss) => return miss,
            },
        };
        let calls: Vec<Node<'a>> = self
            .tagged("reference.call")
            .into_iter()
            .filter(|(call, called)| *called == name || self.callee_text(*call) == name)
            .map(|(call, _)| call)
            .filter(|call| around.is_none_or(|function| contains(function, *call)))
            .collect();
        let what = format!("call `{name}`");
        match calls.as_slice() {
            [] => Resolution::Missing { what },
            [call] => match call.child_by_field_name("arguments") {
                Some(list) => self.in_list(list, position),
                None => refused(&format!("{what} has no argument list")),
            },
            _ => Resolution::Several {
                what,
                count: calls.len(),
            },
        }
    }

    /// A place inside a bracketed list: on its line when the list is written on one line, else on
    /// a line of its own like the items, with the list's separator.
    fn in_list(&self, list: Node<'a>, position: Position) -> Resolution {
        if matches!(position, Position::Before | Position::After) {
            return refused("a list's place is `start of` or `end of`");
        }
        let mut cursor = list.walk();
        let items: Vec<Node<'a>> = list
            .named_children(&mut cursor)
            .filter(|item| !is_comment(*item))
            .collect();
        let (Some(first), Some(last)) = (items.first(), items.last()) else {
            return Resolution::At(Place::InLine {
                row: list.start_position().row,
                column: list.start_position().column + 1,
                prefix: String::new(),
                suffix: String::new(),
            });
        };
        let separator = self.separator_of(&items);
        let one_line = list.start_position().row == list.end_position().row;
        let item_indent = |item: &Node| indent_of(self.line(item.start_position().row));
        match (position, one_line) {
            (Position::Start, true) => {
                in_line_at(*first, true, String::new(), format!("{separator} "))
            }
            (Position::End, true) => {
                in_line_at(*last, false, format!("{separator} "), String::new())
            }
            (Position::Start, false) => {
                lines_at(first.start_position().row, item_indent(first), separator)
            }
            _ => {
                let after_last = self.text[last.end_byte()..].trim_start();
                if after_last.starts_with(&separator) {
                    lines_at(last.end_position().row + 1, item_indent(last), separator)
                } else {
                    let prefix = format!("{separator}\n{}", item_indent(last));
                    in_line_at(*last, false, prefix, String::new())
                }
            }
        }
    }

    /// The text between the list's last two items, trimmed; `,` for a list of one.
    fn separator_of(&self, items: &[Node<'a>]) -> String {
        match items {
            [.., before, last] => {
                let between = self.text[before.end_byte()..last.start_byte()].trim();
                if between.is_empty() {
                    ",".into()
                } else {
                    between.into()
                }
            }
            _ => ",".into(),
        }
    }

    /// The one definition of this kind by this name; a `Class.method` name is a method inside
    /// that class. None, or several, is the miss to say.
    fn the_one(&self, kind: Definition, name: &str) -> std::result::Result<Node<'a>, Resolution> {
        let separator = self.loaded.language.unit_separator.as_str();
        let words: Vec<&str> = if separator.is_empty() {
            vec![name]
        } else {
            name.split(separator).collect()
        };
        let Some((own, outer)) = words.split_last() else {
            return Err(Resolution::Missing {
                what: format!("{} `{name}`", kind.word()),
            });
        };
        let all = self.definitions();
        let found: Vec<Node<'a>> = all
            .iter()
            .filter(|(of, node, named)| {
                *of == kind && named == own && {
                    outer.iter().all(|word| {
                        all.iter().any(|(_, around, around_name)| {
                            around_name == word
                                && around.id() != node.id()
                                && contains(*around, *node)
                        })
                    })
                }
            })
            .map(|(_, node, _)| *node)
            .collect();
        let what = format!("{} `{name}`", kind.word());
        match found.as_slice() {
            [] => Err(Resolution::Missing { what }),
            [one] => Ok(*one),
            _ => Err(Resolution::Several {
                what,
                count: found.len(),
            }),
        }
    }

    /// Every definition `tags.scm` tags, with its kind and name.
    fn definitions(&self) -> Vec<(Definition, Node<'a>, String)> {
        [
            (Definition::Function, "definition.function"),
            (Definition::Class, "definition.class"),
        ]
        .into_iter()
        .flat_map(|(kind, capture)| {
            self.tagged(capture)
                .into_iter()
                .map(move |(node, name)| (kind, node, name))
        })
        .collect()
    }

    /// The nodes `tags.scm` captures as `capture`, each with the text of its match's `@name`.
    fn tagged(&self, capture: &str) -> Vec<(Node<'a>, String)> {
        let Some(query) = self.query("tags.scm") else {
            return Vec::new();
        };
        let names = query.capture_names();
        let mut found = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, self.tree.root_node(), self.text.as_bytes());
        while let Some(hit) = matches.next() {
            let named = |wanted: &str| {
                hit.captures
                    .iter()
                    .find(|c| names[c.index as usize] == wanted)
                    .map(|c| c.node)
            };
            if let (Some(node), Some(name)) = (named(capture), named("name")) {
                found.push((node, self.text[name.byte_range()].to_string()));
            }
        }
        found
    }

    /// A definition's statements or members: what `textobjects.scm` captures as its
    /// `@function.inner`/`@class.inner`, or that node's own items when it captures the block.
    fn body_items(&self, kind: Definition, definition: Node<'a>) -> Vec<Node<'a>> {
        let Some(query) = self.query("textobjects.scm") else {
            return Vec::new();
        };
        let names = query.capture_names();
        let (outer, inner) = (
            format!("{}.outer", kind.word()),
            format!("{}.inner", kind.word()),
        );
        let mut found: Vec<Node<'a>> = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, definition, self.text.as_bytes());
        while let Some(hit) = matches.next() {
            let of_this = hit
                .captures
                .iter()
                .any(|c| names[c.index as usize] == outer && c.node.id() == definition.id());
            if of_this {
                found.extend(
                    hit.captures
                        .iter()
                        .filter(|c| names[c.index as usize] == inner)
                        .map(|c| c.node),
                );
            }
        }
        found.sort_by_key(Node::start_byte);
        found.dedup_by_key(|node| node.id());
        match found.as_slice() {
            [block] if block.named_child_count() > 0 => {
                let mut cursor = block.walk();
                block
                    .named_children(&mut cursor)
                    .filter(|item| !is_comment(*item))
                    .collect()
            }
            _ => found,
        }
    }

    /// Every node a query file captures as `capture`.
    fn captures(&self, file: &str, capture: &str) -> Vec<Node<'a>> {
        let Some(query) = self.query(file) else {
            return Vec::new();
        };
        let Some(index) = query.capture_index_for_name(capture) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, self.tree.root_node(), self.text.as_bytes());
        while let Some(hit) = matches.next() {
            found.extend(
                hit.captures
                    .iter()
                    .filter(|c| c.index == index)
                    .map(|c| c.node),
            );
        }
        found
    }

    fn query(&self, file: &str) -> Option<&'a tree_sitter::Query> {
        self.loaded
            .queries
            .iter()
            .find(|(name, _)| name == file)
            .map(|(_, query)| query)
    }

    /// The ancestor of the node (or the node) that sits directly in the file.
    fn top_level_ancestor(&self, node: Node<'a>) -> Option<Node<'a>> {
        let root = self.tree.root_node();
        std::iter::successors(Some(node), Node::parent).find(|candidate| {
            candidate
                .parent()
                .is_some_and(|parent| parent.id() == root.id())
        })
    }

    /// A call's text up to its arguments: `repo.save` in `repo.save(x)`.
    fn callee_text(&self, call: Node<'a>) -> &'a str {
        let end = call
            .child_by_field_name("arguments")
            .map_or(call.end_byte(), |arguments| arguments.start_byte());
        self.text[call.start_byte()..end].trim()
    }

    fn line(&self, row: usize) -> &'a str {
        self.text.lines().nth(row).unwrap_or("")
    }
}

fn lines_at(row: usize, indent: String, suffix: String) -> Resolution {
    Resolution::At(Place::Lines {
        row,
        indent,
        suffix,
    })
}

/// On the item's line, before it (`before_item`) or after it.
fn in_line_at(item: Node, before_item: bool, prefix: String, suffix: String) -> Resolution {
    let at = if before_item {
        item.start_position()
    } else {
        item.end_position()
    };
    Resolution::At(Place::InLine {
        row: at.row,
        column: at.column,
        prefix,
        suffix,
    })
}

fn contains(outer: Node, inner: Node) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

/// Comments by the grammar's own naming (`comment`, `line_comment`, `block_comment`).
fn is_comment(node: Node) -> bool {
    node.kind().contains("comment")
}

/// A statement that is only a string: a docstring.
fn is_only_a_string(node: Node) -> bool {
    node.named_child_count() == 1
        && node
            .named_child(0)
            .is_some_and(|child| child.kind().contains("string"))
}
