//! Anchor phrases resolved with the language's own query captures (see
//! `sorcerer_core::domain::captures`): `imports.scm`'s `@import.path` for the imports, the
//! `@definition.*` and `@reference.call` captures for definitions and calls by name,
//! `textobjects.scm`'s `@function.inner`/`@class.inner`, `@parameter.inner` and `@comment.outer`
//! for bodies, list items and comments, and `lists.scm`'s `@parameters`/`@arguments` for the lists.
//! A language gets anchors by having those captures; nothing here names a language or a node kind.

use std::path::Path;

use sorcerer_core::Result;
use sorcerer_core::domain::captures::{ARGUMENTS, COMMENT, Captures, DOCSTRING, PARAMETERS};
use sorcerer_core::domain::language::Loaded;
use tree_sitter::{Node, Tree};

use crate::domain::anchor::{
    Anchor, Place, Position, Resolution, Target, indent_of, resolve_in_text,
};
use crate::domain::language_syntax::list_separator;
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
            captures: Captures::of(&loaded, &tree, text),
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

/// What the queries tag a definition as: `@definition.function` or `@definition.method` is a
/// function, `@definition.class` a class.
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

    /// The definition a capture names, or `None` for one that only holds others (a module, an
    /// impl).
    fn of_capture(capture: &str) -> Option<Definition> {
        match capture.strip_prefix("definition.")? {
            "function" | "method" => Some(Definition::Function),
            "class" => Some(Definition::Class),
            _ => None,
        }
    }
}

/// One definition the queries tag: what it is (if a phrase can name it), its node, its name.
struct Tagged<'a> {
    kind: Option<Definition>,
    node: Node<'a>,
    name: String,
}

/// A file parsed in its language, with what its queries capture.
struct Parsed<'a> {
    loaded: &'a Loaded,
    tree: &'a Tree,
    text: &'a str,
    captures: Captures<'a>,
}

impl<'a> Parsed<'a> {
    /// `end of imports`: after the last top-level import; `start of`: before the first. A file
    /// with none takes them after its leading comments and docstring.
    fn imports(&self, position: Position) -> Resolution {
        let imports = self.top_level_imports();
        let row = match (imports.first(), imports.last(), position) {
            (Some(first), _, Position::Start | Position::Before) => first.start_position().row,
            (_, Some(last), _) => row_after(*last),
            _ => self.row_after_the_preamble(),
        };
        lines_at(row, String::new(), String::new())
    }

    /// The top-level statements holding an `@import.path`, in file order. A whole definition
    /// (a function, a class, a module block) holds its imports inside, so it is not one.
    fn top_level_imports(&self) -> Vec<Node<'a>> {
        let mut statements: Vec<Node<'a>> = self
            .captures
            .nodes(&["import.path"])
            .into_iter()
            .filter_map(|capture| self.top_level_ancestor(capture))
            .filter(|statement| !self.is_a_whole_definition(*statement))
            .collect();
        statements.sort_by_key(Node::start_byte);
        statements.dedup_by_key(|statement| statement.id());
        statements
    }

    fn is_a_whole_definition(&self, node: Node<'a>) -> bool {
        self.captures.is("function.outer", node) || self.captures.is("class.outer", node)
    }

    /// The row after the file's leading comments and its docstring (`@comment.outer`,
    /// `@docstring`), or 0.
    fn row_after_the_preamble(&self) -> usize {
        let root = self.tree.root_node();
        let mut cursor = root.walk();
        root.named_children(&mut cursor)
            .take_while(|node| {
                self.captures.is(COMMENT, *node) || self.captures.is(DOCSTRING, *node)
            })
            .last()
            .map_or(0, row_after)
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
                row_after(definition),
                indent_at(definition.start_position().row),
                String::new(),
            ),
            Position::Start | Position::End => {
                let items = self.captures.body_items(definition);
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
                        row_after(*item),
                        indent_at(item.start_position().row),
                        String::new(),
                    ),
                }
            }
        }
    }

    /// Inside the function's parameter list (its own `@parameters`).
    fn parameters(&self, name: &str, position: Position) -> Resolution {
        let function = match self.the_one(Definition::Function, name) {
            Ok(function) => function,
            Err(miss) => return miss,
        };
        match self.captures.own(PARAMETERS, function) {
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
            .captures
            .named("reference.call", self.text)
            .into_iter()
            .filter(|(call, called)| *called == name || self.callee_text(*call) == name)
            .map(|(call, _)| call)
            .filter(|call| around.is_none_or(|function| contains(function, *call)))
            .collect();
        let what = format!("call `{name}`");
        match calls.as_slice() {
            [] => Resolution::Missing { what },
            [call] => match self.arguments_of(*call) {
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
        let items = self.captures.list_items(list);
        let Some(separator) = self.separator_of(&items) else {
            return refused(&format!(
                "a list of one item or none takes the separator `[syntax] list_separator` names, and the language {} names none",
                self.loaded.language.name
            ));
        };
        let (Some(first), Some(last)) = (items.first(), items.last()) else {
            return Resolution::At(self.inside_an_empty_list(list));
        };
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
                    lines_at(row_after(*last), item_indent(last), separator)
                } else {
                    let prefix = format!("{separator}\n{}", item_indent(last));
                    in_line_at(*last, false, prefix, String::new())
                }
            }
        }
    }

    /// Right after the list's opening token (its first child), on its line.
    fn inside_an_empty_list(&self, list: Node<'a>) -> Place {
        let at = list
            .child(0)
            .map_or(list.start_position(), |open| open.end_position());
        Place::InLine {
            row: at.row,
            column: at.column,
            prefix: String::new(),
            suffix: String::new(),
        }
    }

    /// The text between the list's last two items, trimmed; for a list of one or none, the
    /// language's `[syntax] list_separator`.
    fn separator_of(&self, items: &[Node<'a>]) -> Option<String> {
        let between = match items {
            [.., before, last] => self.text[before.end_byte()..last.start_byte()].trim(),
            _ => "",
        };
        if between.is_empty() {
            list_separator(&self.loaded.language)
        } else {
            Some(between.into())
        }
    }

    /// The one definition of this kind by this name; a `Class.method` name (with the language's
    /// `unit_separator`) is a method inside that class. None, or several, is the miss to say.
    fn the_one(&self, kind: Definition, name: &str) -> std::result::Result<Node<'a>, Resolution> {
        let separator = self.loaded.language.unit_separator.as_str();
        let words: Vec<&str> = if separator.is_empty() {
            vec![name]
        } else {
            name.split(separator).collect()
        };
        let what = format!("{} `{name}`", kind.word());
        let Some((own, outer)) = words.split_last() else {
            return Err(Resolution::Missing { what });
        };
        let all = self.definitions();
        let found: Vec<Node<'a>> = all
            .iter()
            .filter(|tagged| {
                tagged.kind == Some(kind)
                    && tagged.name == *own
                    && outer.iter().all(|word| {
                        all.iter().any(|around| {
                            around.name == *word
                                && around.node.id() != tagged.node.id()
                                && contains(around.node, tagged.node)
                        })
                    })
            })
            .map(|tagged| tagged.node)
            .collect();
        match found.as_slice() {
            [] => Err(Resolution::Missing { what }),
            [one] => Ok(*one),
            _ => Err(Resolution::Several {
                what,
                count: found.len(),
            }),
        }
    }

    /// Every definition the queries tag (`@definition.<kind>` with its `@name`), each node once
    /// for each kind.
    fn definitions(&self) -> Vec<Tagged<'a>> {
        let mut found: Vec<Tagged<'a>> = self
            .captures
            .matches()
            .iter()
            .filter_map(|captures| {
                let (capture, node) = captures
                    .iter()
                    .find(|(capture, _)| capture.starts_with("definition."))?;
                let name = captures.iter().find(|(capture, _)| capture == "name")?.1;
                Some(Tagged {
                    kind: Definition::of_capture(capture),
                    node: *node,
                    name: self.text[name.byte_range()].to_string(),
                })
            })
            .collect();
        found.sort_by_key(|tagged| (tagged.node.start_byte(), tagged.node.id()));
        found.dedup_by(|a, b| a.node.id() == b.node.id() && a.kind == b.kind);
        found
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

    /// A call's `@arguments` list: the one that is the call's own child.
    fn arguments_of(&self, call: Node<'a>) -> Option<Node<'a>> {
        self.captures
            .nodes(&[ARGUMENTS])
            .into_iter()
            .find(|list| list.parent().is_some_and(|parent| parent.id() == call.id()))
    }

    /// A call's text up to its arguments: `repo.save` in `repo.save(x)`.
    fn callee_text(&self, call: Node<'a>) -> &'a str {
        let end = self
            .arguments_of(call)
            .map_or(call.end_byte(), |arguments| arguments.start_byte());
        self.text[call.start_byte()..end].trim()
    }

    fn line(&self, row: usize) -> &'a str {
        self.text.lines().nth(row).unwrap_or("")
    }
}

/// The row after a node's last line. A node that ends at the start of a line (a comment that
/// holds its line ending) ends on the line before.
fn row_after(node: Node) -> usize {
    let (start, end) = (node.start_position(), node.end_position());
    if end.column == 0 && end.row > start.row {
        end.row
    } else {
        end.row + 1
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
