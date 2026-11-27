//! The calls and definitions in a text, by the language's queries: what a rules retry is
//! compared on. No language is named here; a language whose queries capture no
//! `@reference.call` simply has no calls.

use sorcerer_core::domain::captures::{ARGUMENTS, Captures};
use sorcerer_core::domain::language::Loaded;
use sorcerer_core::domain::unit::units_of;
use tree_sitter::Node;

use crate::domain::retry_drops::{NameKind, NamedInText};

/// What `@reference.call` marks: a call (or a struct literal).
const CALL: &str = "reference.call";

/// Every call and definition in the text, in file order; none when the parser gives up.
///
/// A call is named by its callee as written, its argument lists shortened to their brackets
/// and its spaces squeezed: `std::fs::write("a", b)` is `std::fs::write`, and
/// `db.connect("x").total()` is `db.connect().total`. A call with no `@arguments` (a macro)
/// is named up to its `@name`. A definition is named as its unit is (`Order.cancel`).
pub fn names_in(lang: &Loaded, text: &str) -> Vec<NamedInText> {
    let Ok(Some(tree)) = lang.parse(text) else {
        return Vec::new();
    };
    let captures = Captures::of(lang, &tree, text);
    let argument_lists = captures.nodes(&[ARGUMENTS]);
    let calls = captures
        .named(CALL, text)
        .into_iter()
        .filter_map(|(call, name)| {
            Some(NamedInText {
                kind: NameKind::Call,
                name: callee_of(call, &name, &argument_lists, text)?,
                line: call.start_position().row as u32 + 1,
                column: call.start_position().column as u32,
            })
        });
    let definitions = units_of(lang, text)
        .unwrap_or_default()
        .into_iter()
        .map(|unit| NamedInText {
            kind: NameKind::Definition,
            name: unit.name,
            line: unit.span.start,
            column: 0,
        });
    let mut found: Vec<NamedInText> = calls.chain(definitions).collect();
    found.sort_by_key(|named| (named.line, named.column));
    found
}

/// The call's text before its own argument list (or up to its name, when it has none), with
/// every argument list inside it cut to its brackets and its spaces squeezed.
fn callee_of(call: Node, name: &str, argument_lists: &[Node], text: &str) -> Option<String> {
    let inside =
        |node: &Node| node.start_byte() >= call.start_byte() && node.end_byte() <= call.end_byte();
    let own_list = argument_lists
        .iter()
        .filter(|list| inside(list) && list.end_byte() == call.end_byte())
        .min_by_key(|list| list.start_byte());
    let end = match own_list {
        Some(list) => list.start_byte(),
        None => call.start_byte() + text[call.byte_range()].find(name)? + name.len(),
    };
    let inner_lists: Vec<&Node> = argument_lists
        .iter()
        .filter(|list| list.start_byte() >= call.start_byte() && list.end_byte() <= end)
        .collect();
    let mut callee = String::new();
    let mut at = call.start_byte();
    for list in inner_lists {
        if list.start_byte() < at {
            continue;
        }
        callee.push_str(&text[at..list.start_byte()]);
        callee.push_str(&brackets_of(&text[list.byte_range()]));
        at = list.end_byte();
    }
    callee.push_str(&text[at..end]);
    let squeezed: String = callee.split_whitespace().collect();
    (!squeezed.is_empty()).then_some(squeezed)
}

/// An argument list's opening and closing characters: `("a", b)` is `()`.
fn brackets_of(list: &str) -> String {
    let mut chars = list.chars();
    match (chars.next(), chars.next_back()) {
        (Some(open), Some(close)) => format!("{open}{close}"),
        _ => String::new(),
    }
}
