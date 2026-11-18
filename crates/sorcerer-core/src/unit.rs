//! Units: the named definitions a person would point at (a function, a
//! method, a class, an impl), found by the language's queries. The rules
//! are olifant's (`definitionsWith` in `Language/Captures.hs`, `qualify` in
//! `Server/Unit.hs`):
//!
//! 1. A definition is a capture named `definition.<kind>` (a kind in
//!    [`UNIT_KINDS`]) or `unit.<anything>` (but `unit.name`), in a match that
//!    also captures `@name` (or `@unit.name`). Its bare name is those names'
//!    texts in source order, joined by a space. A node captured twice is one
//!    definition.
//! 2. Its span is widened to the earliest-starting `@function.outer` or
//!    `@class.outer` that contains it, ends where it ends, and contains no
//!    other definition except ones inside it (a decorator is in; a class's
//!    range is the class's).
//! 3. Its name is qualified one level: the bare name of the innermost other
//!    unit whose lines contain it, then the language's `unit_separator`.

use tree_sitter::{Node, Parser, QueryCursor, StreamingIterator};

use crate::fingerprint::{Fingerprint, fingerprint};
use crate::language::Loaded;
use crate::{Error, Result};

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

/// 1-based lines, both ends included.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn contains(&self, other: &Span) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    pub fn holds(&self, line: u32) -> bool {
        self.start <= line && line <= self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum UnitKind {
    Function,
    Method,
    Class,
    Impl,
    Type,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Unit {
    /// Qualified one level: `Order.cancel`, `Order::new`.
    pub name: String,
    pub kind: UnitKind,
    /// The whole definition, decorators in.
    pub span: Span,
    /// The grammar's `body` field, when it has one.
    pub body: Option<Span>,
    pub fingerprint: Fingerprint,
}

/// A definition as the queries found it.
struct Def {
    capture: String,
    bytes: (usize, usize),
    lines: Span,
    name: String,
    body: Option<Span>,
}

/// The lines a node covers. A node ending at column 0 ends on the line before.
fn lines_of(n: &Node) -> Span {
    let (s, e) = (n.start_position(), n.end_position());
    let end = if e.column == 0 && e.row > s.row {
        e.row
    } else {
        e.row + 1
    };
    Span {
        start: s.row as u32 + 1,
        end: end as u32,
    }
}

fn within(outer: (usize, usize), inner: (usize, usize)) -> bool {
    outer.0 <= inner.0 && inner.1 <= outer.1
}

/// The units of TEXT, in source order (start ascending, then end descending).
pub fn units_of(lang: &Loaded, text: &str) -> Result<Vec<Unit>> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.grammar)
        .map_err(|e| Error::GrammarLoad {
            name: lang.language.name.clone(),
            why: e.to_string(),
        })?;
    let Some(tree) = parser.parse(text, None) else {
        return Ok(Vec::new());
    };
    let source = text.as_bytes();
    let mut defs: Vec<Def> = Vec::new();
    let mut outers: Vec<((usize, usize), Span)> = Vec::new();
    for (_, query) in &lang.queries {
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), source);
        while let Some(m) = matches.next() {
            let mut name_parts: Vec<(usize, &str)> = Vec::new();
            let mut def: Option<(String, Node)> = None;
            for c in m.captures {
                let cap = names[c.index as usize];
                match cap {
                    "name" | "unit.name" => name_parts.push((
                        c.node.start_byte(),
                        c.node.utf8_text(source).unwrap_or_default(),
                    )),
                    "function.outer" | "class.outer" => {
                        outers.push(((c.node.start_byte(), c.node.end_byte()), lines_of(&c.node)))
                    }
                    _ if is_unit_capture(cap) => def = Some((cap.to_string(), c.node)),
                    _ => {}
                }
            }
            if let Some((capture, node)) = def
                && !name_parts.is_empty()
            {
                name_parts.sort();
                defs.push(Def {
                    capture,
                    bytes: (node.start_byte(), node.end_byte()),
                    lines: lines_of(&node),
                    name: name_parts
                        .iter()
                        .map(|(_, t)| *t)
                        .collect::<Vec<_>>()
                        .join(" "),
                    body: node.child_by_field_name("body").map(|b| lines_of(&b)),
                });
            }
        }
    }
    // one definition per node: the first found, in source order
    defs.sort_by_key(|d| d.bytes);
    defs.dedup_by_key(|d| d.bytes);
    let def_bytes: Vec<_> = defs.iter().map(|d| d.bytes).collect();
    let widened: Vec<Span> = defs
        .iter()
        .map(|d| {
            outers
                .iter()
                .filter(|(o, _)| within(*o, d.bytes) && o.1 == d.bytes.1)
                .filter(|(o, _)| {
                    def_bytes
                        .iter()
                        .all(|x| within(d.bytes, *x) || !within(*o, *x))
                })
                .min_by_key(|(o, _)| o.0)
                .map(|(_, l)| *l)
                .unwrap_or(d.lines)
        })
        .collect();
    let sep = &lang.language.unit_separator;
    let mut units: Vec<Unit> = defs
        .iter()
        .zip(&widened)
        .enumerate()
        .map(|(i, (d, span))| {
            let parent = innermost_parent(&widened, i);
            let kind = kind_of(&d.capture, parent.map(|p| kind_of(&defs[p].capture, None)));
            let name = match parent {
                Some(p) => format!("{}{sep}{}", defs[p].name, d.name),
                None => d.name.clone(),
            };
            Unit {
                name,
                kind,
                span: *span,
                body: d.body,
                fingerprint: fingerprint(text, *span),
            }
        })
        .collect();
    units.sort_by_key(|u| (u.span.start, std::cmp::Reverse(u.span.end)));
    Ok(units)
}

fn is_unit_capture(cap: &str) -> bool {
    match cap.strip_prefix("definition.") {
        Some(kind) => UNIT_KINDS.contains(&kind),
        None => cap.starts_with("unit.") && cap != "unit.name",
    }
}

/// The innermost other span that contains span I (the latest start, then the earliest end).
fn innermost_parent(spans: &[Span], i: usize) -> Option<usize> {
    let me = spans[i];
    (0..spans.len())
        .filter(|&j| j != i && spans[j] != me && spans[j].contains(&me))
        .max_by_key(|&j| (spans[j].start, std::cmp::Reverse(spans[j].end)))
}

/// A unit's kind from its capture; a function inside a class, impl or type is a method.
fn kind_of(capture: &str, parent: Option<UnitKind>) -> UnitKind {
    let suffix = capture.split_once('.').map_or(capture, |(_, s)| s);
    match (capture, suffix) {
        (_, "function")
            if matches!(
                parent,
                Some(UnitKind::Class | UnitKind::Impl | UnitKind::Type)
            ) =>
        {
            UnitKind::Method
        }
        (_, "function") => UnitKind::Function,
        (_, "method") => UnitKind::Method,
        (_, "class") => UnitKind::Class,
        (_, "impl") => UnitKind::Impl,
        (_, "struct" | "enum" | "trait" | "type" | "interface" | "union") => UnitKind::Type,
        (_, other) => UnitKind::Other(other.to_string()),
    }
}

/// The innermost unit whose span holds LINE (1-based).
pub fn unit_at(units: &[Unit], line: u32) -> Option<&Unit> {
    units
        .iter()
        .filter(|u| u.span.holds(line))
        .min_by_key(|u| u.span.end - u.span.start)
}
