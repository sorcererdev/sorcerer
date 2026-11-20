//! Compiling a language's tree-sitter query files.

use tree_sitter::{Language, Query};

use crate::{Error, Result};

/// Compiles the text of one query file (such as `units.scm`) for a grammar, and refuses a
/// query that uses a predicate tree-sitter would silently skip.
///
/// The `tree-sitter` crate checks only the text predicates itself: `#eq?`, `#match?`,
/// `#any-of?` and their `#not-` forms. Any other predicate or directive (`#set!`, `#is?`, a
/// made-up `#has-parent?`) is ignored without a word, so the pattern would match code its
/// author meant to leave out. Failing loudly is safer.
///
/// `file` is the query file's name, used only in the error. Example: a pattern ending in
/// `(#has-parent? @name block)` in `units.scm` fails with "units.scm: unsupported predicate
/// #has-parent?; remove the pattern or rewrite it with #eq?, #match? or #any-of?".
pub fn compile(grammar: &Language, file: &str, text: &str) -> Result<Query> {
    let query = Query::new(grammar, text)
        .map_err(|e| query_error(file, format!("line {}: {}", e.row + 1, e.message)))?;
    match first_unchecked_predicate(&query) {
        Some(name) => Err(query_error(file, unsupported_predicate_text(&name))),
        None => Ok(query),
    }
}

/// The name of the first predicate or directive, across all the query's patterns, that
/// tree-sitter does not check itself; `None` when every one is checked.
fn first_unchecked_predicate(query: &Query) -> Option<String> {
    (0..query.pattern_count()).find_map(|pattern| unchecked_predicates(query, pattern).next())
}

/// One pattern's unchecked predicates: made-up ones, `#set!` directives, and `#is?` or
/// `#is-not?` property checks.
fn unchecked_predicates(query: &Query, pattern: usize) -> impl Iterator<Item = String> + '_ {
    let made_up = query
        .general_predicates(pattern)
        .iter()
        .map(|predicate| predicate.operator.to_string());
    let settings = query
        .property_settings(pattern)
        .iter()
        .map(|_| "set!".to_string());
    let property_checks = query
        .property_predicates(pattern)
        .iter()
        .map(|(_, positive)| if *positive { "is?" } else { "is-not?" }.to_string());
    made_up.chain(settings).chain(property_checks)
}

fn unsupported_predicate_text(name: &str) -> String {
    format!(
        "unsupported predicate #{name}; remove the pattern or rewrite it with #eq?, #match? or #any-of?"
    )
}

fn query_error(file: &str, why: String) -> Error {
    Error::Query {
        file: file.to_string(),
        why,
    }
}
