//! Compiling a language's tree-sitter queries. The `tree-sitter` crate
//! evaluates the text predicates itself (`#eq?`, `#match?`, `#any-of?` and
//! their `#not-` forms); any other predicate or directive would be
//! silently ignored, which makes a query match what its author did not
//! mean, so it fails the file's compile with its name.

use tree_sitter::{Language, Query};

use crate::{Error, Result};

pub fn compile(grammar: &Language, file: &str, text: &str) -> Result<Query> {
    let bad = |why: String| Error::Query {
        file: file.to_string(),
        why,
    };
    let q =
        Query::new(grammar, text).map_err(|e| bad(format!("line {}: {}", e.row + 1, e.message)))?;
    for i in 0..q.pattern_count() {
        let named = q
            .general_predicates(i)
            .iter()
            .map(|p| p.operator.to_string())
            .chain(q.property_settings(i).iter().map(|_| "set!".to_string()))
            .chain(
                q.property_predicates(i)
                    .iter()
                    .map(|(_, positive)| if *positive { "is?" } else { "is-not?" }.to_string()),
            )
            .next();
        if let Some(op) = named {
            return Err(bad(format!(
                "unsupported predicate #{op}; remove the pattern or rewrite it with #eq?, #match? or #any-of?"
            )));
        }
    }
    Ok(q)
}
