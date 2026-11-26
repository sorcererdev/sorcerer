//! Units: the named definitions a person would point at, such as a function, a method, a
//! class or an impl block. The language's query files find them; this file turns what the
//! queries captured into a list of units. The rules:
//!
//! 1. A definition is a capture named `definition.<kind>` (a kind in [`UNIT_KINDS`]) or
//!    `unit.<anything>` (except `unit.name`), in a match that also captures `@name` (or
//!    `@unit.name`). Its name is the text of those name captures in file order, joined by a
//!    space. A node captured twice is one definition.
//! 2. Its lines are widened to take in its decorators or attributes: to the earliest-starting
//!    `@function.outer` or `@class.outer` that holds it, ends where it ends, and holds no other
//!    definition except ones inside it.
//! 3. Its name is qualified one level: the name of the innermost other unit around it, then the
//!    language's `unit_separator`, then its own name (`Order.cancel`, `Order::new`).

use tree_sitter::Node;

use crate::Result;
use crate::domain::captures::{CapturedMatch, Captures};
use crate::domain::fingerprint::{Fingerprint, fingerprint};
use crate::domain::language::Loaded;

/// The `definition.<kind>` captures that are units.
pub const UNIT_KINDS: &[&str] = &[
    "function",
    "method",
    "class",
    "interface",
    "module",
    "type",
    "impl",
    "instance",
];

/// A range of lines, counted from 1, both ends included: lines 3 to 5 are three lines.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// Whether every line of the other span is also in this one.
    pub fn contains(&self, other: &Span) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// Whether the line is in this span.
    pub fn holds(&self, line: u32) -> bool {
        self.start <= line && line <= self.end
    }
}

/// What sort of definition a unit is. A function defined inside a class, impl or type is a
/// `Method`; a kind sorcerer has no name for keeps the query's word (`Other("module")`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum UnitKind {
    Function,
    Method,
    Class,
    Impl,
    Type,
    /// Not a definition: the lines the parser could not read, standing where a unit would
    /// (see `broken_region`).
    BrokenRegion,
    Other(String),
}

/// One named definition in a file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Unit {
    /// The name, qualified one level by the unit around it: `Order.cancel`, `Order::new`.
    pub name: String,
    pub kind: UnitKind,
    /// All the definition's lines, its decorators or attributes included.
    pub span: Span,
    /// The lines of its body, when `textobjects.scm` captures one (`@function.inner` or
    /// `@class.inner` inside the definition): `None` for a definition with nothing in its body.
    pub body: Option<Span>,
    /// Names the unit's text; see [`fingerprint`].
    pub fingerprint: Fingerprint,
}

/// Finds the units of a file, in file order: by first line, and a unit before the units
/// inside it.
///
/// Example: a file holding class `Order` (lines 1-9) with a decorated method `cancel` (lines
/// 4-9, the decorator on line 4) gives `Order` (lines 1-9, a class), then `Order.cancel`
/// (lines 4-9, a method), with `.` as the language's separator. When the parser gives up on the
/// file, there are no units.
pub fn units_of(lang: &Loaded, text: &str) -> Result<Vec<Unit>> {
    let Some(tree) = lang.parse(text)? else {
        return Ok(Vec::new());
    };
    let captures = Captures::of(lang, &tree, text);
    let captured = Captured::from(&captures, text);
    let definitions = one_per_node(captured.definitions);
    let spans: Vec<Span> = definitions
        .iter()
        .map(|definition| widened_span(definition, &definitions, &captured.outer_ranges))
        .collect();
    let units = named_units(&definitions, &spans, &lang.language.unit_separator, text);
    Ok(in_file_order(units))
}

/// Where a node starts and ends in the file, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ByteRange {
    start: usize,
    end: usize,
}

impl ByteRange {
    fn of(node: &Node) -> ByteRange {
        ByteRange {
            start: node.start_byte(),
            end: node.end_byte(),
        }
    }

    fn contains(self, inner: ByteRange) -> bool {
        self.start <= inner.start && inner.end <= self.end
    }
}

/// A definition as a query captured it, before it becomes a unit.
struct Definition {
    /// The capture's name, such as `definition.function`.
    capture: String,
    bytes: ByteRange,
    lines: Span,
    /// The bare name, not yet qualified.
    name: String,
    body: Option<Span>,
}

impl Definition {
    /// A definition from its node, its name captures (each with where it starts, in bytes) and
    /// what the queries captured as its body.
    fn new(
        capture: &str,
        node: &Node,
        mut name_parts: Vec<(usize, &str)>,
        body: &[Node],
    ) -> Definition {
        name_parts.sort();
        let name: Vec<&str> = name_parts.iter().map(|(_, part)| *part).collect();
        Definition {
            capture: capture.to_string(),
            bytes: ByteRange::of(node),
            lines: lines_of(node),
            name: name.join(" "),
            body: lines_of_all(body),
        }
    }
}

/// The lines from the first node's first to the last node's last; `None` for no nodes.
fn lines_of_all(nodes: &[Node]) -> Option<Span> {
    let first = nodes.iter().map(|node| lines_of(node).start).min()?;
    let last = nodes.iter().map(|node| lines_of(node).end).max()?;
    Some(Span {
        start: first,
        end: last,
    })
}

/// A `@function.outer` or `@class.outer` capture: a range that may widen a definition.
struct OuterRange {
    bytes: ByteRange,
    lines: Span,
}

/// Everything the language's queries captured in one file.
#[derive(Default)]
struct Captured {
    definitions: Vec<Definition>,
    outer_ranges: Vec<OuterRange>,
}

impl Captured {
    /// Gathers the definitions and outer ranges from every match the language's queries made.
    fn from(captures: &Captures, text: &str) -> Captured {
        let mut captured = Captured::default();
        for found in captures.matches() {
            captured.add_match(captures, found, text);
        }
        captured
    }

    /// Records one query match: any outer ranges in it, and its definition when the match
    /// also captured a name. When a match captures several definitions, the last one counts.
    fn add_match(&mut self, captures: &Captures, found: &CapturedMatch<'_>, text: &str) {
        let mut name_parts: Vec<(usize, &str)> = Vec::new();
        let mut definition: Option<(&str, Node)> = None;
        for (capture, node) in found {
            match capture.as_str() {
                "name" | "unit.name" => name_parts.push((
                    node.start_byte(),
                    node.utf8_text(text.as_bytes()).unwrap_or_default(),
                )),
                "function.outer" | "class.outer" => self.outer_ranges.push(OuterRange {
                    bytes: ByteRange::of(node),
                    lines: lines_of(node),
                }),
                name if is_unit_capture(name) => definition = Some((name, *node)),
                _ => {}
            }
        }
        if let Some((capture, node)) = definition
            && !name_parts.is_empty()
        {
            let body = captures.body_of(node);
            self.definitions
                .push(Definition::new(capture, &node, name_parts, &body));
        }
    }
}

/// Whether a capture names a unit: `definition.<kind>` with a kind in [`UNIT_KINDS`], or
/// `unit.<anything>` but `unit.name`.
pub(crate) fn is_unit_capture(capture: &str) -> bool {
    match capture.strip_prefix("definition.") {
        Some(kind) => UNIT_KINDS.contains(&kind),
        None => capture.starts_with("unit.") && capture != "unit.name",
    }
}

/// The definitions sorted by where they start, keeping only the first found for a node that
/// two queries (or two patterns) both captured.
fn one_per_node(mut definitions: Vec<Definition>) -> Vec<Definition> {
    definitions.sort_by_key(|definition| definition.bytes);
    definitions.dedup_by_key(|definition| definition.bytes);
    definitions
}

/// The definition's lines, widened to take in its decorators or attributes.
///
/// The widest candidate wins: the earliest-starting outer range that holds the definition, ends
/// where it ends, and holds no other definition unless that one is inside this definition too.
/// With no such range, the definition's own lines.
fn widened_span(definition: &Definition, all: &[Definition], outers: &[OuterRange]) -> Span {
    outers
        .iter()
        .filter(|outer| {
            outer.bytes.contains(definition.bytes) && outer.bytes.end == definition.bytes.end
        })
        .filter(|outer| {
            all.iter().all(|other| {
                definition.bytes.contains(other.bytes) || !outer.bytes.contains(other.bytes)
            })
        })
        .min_by_key(|outer| outer.bytes.start)
        .map_or(definition.lines, |outer| outer.lines)
}

/// Turns each definition, with its widened span, into a unit: its name qualified by the
/// innermost unit around it, its kind, and its fingerprint.
fn named_units(
    definitions: &[Definition],
    spans: &[Span],
    separator: &str,
    text: &str,
) -> Vec<Unit> {
    definitions
        .iter()
        .zip(spans)
        .enumerate()
        .map(|(i, (definition, span))| {
            let parent = innermost_parent(spans, i).map(|p| &definitions[p]);
            let parent_kind = parent.map(|parent| kind_of(&parent.capture, None));
            let name = match parent {
                Some(parent) => format!("{}{separator}{}", parent.name, definition.name),
                None => definition.name.clone(),
            };
            Unit {
                name,
                kind: kind_of(&definition.capture, parent_kind),
                span: *span,
                body: definition.body,
                fingerprint: fingerprint(text, *span),
            }
        })
        .collect()
}

/// The index of the innermost other span that contains span `i`: the one that starts last,
/// and of those the one that ends first. A span identical to `i`'s does not count.
fn innermost_parent(spans: &[Span], i: usize) -> Option<usize> {
    let me = spans[i];
    (0..spans.len())
        .filter(|&j| j != i && spans[j] != me && spans[j].contains(&me))
        .max_by_key(|&j| (spans[j].start, std::cmp::Reverse(spans[j].end)))
}

/// A unit's kind from its capture's last word; a function inside a class, impl or type is a
/// method.
fn kind_of(capture: &str, parent: Option<UnitKind>) -> UnitKind {
    let word = capture.split_once('.').map_or(capture, |(_, word)| word);
    match word {
        "function"
            if matches!(
                parent,
                Some(UnitKind::Class | UnitKind::Impl | UnitKind::Type)
            ) =>
        {
            UnitKind::Method
        }
        "function" => UnitKind::Function,
        "method" => UnitKind::Method,
        "class" => UnitKind::Class,
        "impl" => UnitKind::Impl,
        "struct" | "enum" | "trait" | "type" | "interface" | "union" => UnitKind::Type,
        other => UnitKind::Other(other.to_string()),
    }
}

/// Sorted by first line; of two units starting on one line, the longer (the outer) first.
fn in_file_order(mut units: Vec<Unit>) -> Vec<Unit> {
    units.sort_by_key(|unit| (unit.span.start, std::cmp::Reverse(unit.span.end)));
    units
}

/// The lines a node covers. A node ending at column 0 of a line really ends on the line
/// before (its last character is the newline).
pub(crate) fn lines_of(node: &Node) -> Span {
    let (start, end) = (node.start_position(), node.end_position());
    let end_line = if end.column == 0 && end.row > start.row {
        end.row
    } else {
        end.row + 1
    };
    Span {
        start: start.row as u32 + 1,
        end: end_line as u32,
    }
}

/// The innermost unit holding the line (the shortest one), or `None` when the line is
/// outside every unit.
pub fn unit_at(units: &[Unit], line: u32) -> Option<&Unit> {
    units
        .iter()
        .filter(|unit| unit.span.holds(line))
        .min_by_key(|unit| unit.span.end - unit.span.start)
}
