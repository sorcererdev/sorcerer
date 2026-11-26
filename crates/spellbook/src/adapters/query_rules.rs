//! Rules run as tree-sitter queries: a rule's query is compiled once for each language and run
//! over the file's tree; each `@finding` capture is a finding at the line its node starts on.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use sorcerer_core::domain::captures::{COMMENT, Captures};
use sorcerer_core::domain::query::compile;
use sorcerer_core::{Error, Result};
use tree_sitter::{Query, QueryCursor, StreamingIterator};

use crate::domain::allow::Comment;
use crate::domain::finding::{RawFinding, Span};
use crate::domain::rule::{Rule, RuleBody};
use crate::domain::rule_query::query_text;
use crate::ports::grammars::Grammars;
use crate::ports::rule_runner::RuleRunner;

/// The capture a rule's query names the nodes it reports with.
const FINDING_CAPTURE: &str = "finding";

pub struct QueryRules<'g> {
    grammars: &'g dyn Grammars,
    /// The compiled queries, by language and rule id.
    compiled: Mutex<HashMap<(String, String), Arc<Query>>>,
}

impl<'g> QueryRules<'g> {
    pub fn new(grammars: &'g dyn Grammars) -> QueryRules<'g> {
        QueryRules {
            grammars,
            compiled: Mutex::new(HashMap::new()),
        }
    }

    /// The rule's query for a language, compiled the first time it is asked for.
    fn query_for(
        &self,
        language: &sorcerer_core::domain::language::Loaded,
        rule: &Rule,
        file: &Path,
    ) -> Result<Arc<Query>> {
        let key = (language.language.name.clone(), rule.id.clone());
        let mut compiled = self
            .compiled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(query) = compiled.get(&key) {
            return Ok(query.clone());
        }
        let name = format!("rule {} on {}", rule.id, file.display());
        let text = match &rule.body {
            RuleBody::Match(form) => query_text(form),
            RuleBody::Query(text) => text.clone(),
        };
        let query = compile(&language.grammar, &name, &text)?;
        if !query.capture_names().contains(&FINDING_CAPTURE) {
            return Err(Error::Query {
                file: format!("rule {}", rule.id),
                why:
                    "its query has no `@finding` capture; capture the node to report as `@finding`"
                        .into(),
            });
        }
        let query = Arc::new(query);
        compiled.insert(key, query.clone());
        Ok(query)
    }
}

impl RuleRunner for QueryRules<'_> {
    fn findings_in(&self, file: &Path, text: &str, rules: &[&Rule]) -> Result<Vec<RawFinding>> {
        let Some(loaded) = self.grammars.for_path(file)? else {
            return Ok(Vec::new());
        };
        let Some(tree) = loaded.parse(text)? else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for rule in rules {
            let query = self.query_for(&loaded, rule, file)?;
            let capture = query
                .capture_index_for_name(FINDING_CAPTURE)
                .unwrap_or_default();
            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(&query, tree.root_node(), text.as_bytes());
            while let Some(found_match) = matches.next() {
                for hit in found_match.captures.iter().filter(|c| c.index == capture) {
                    found.push(RawFinding {
                        rule: rule.id.clone(),
                        line: u32::try_from(hit.node.start_position().row + 1).unwrap_or(u32::MAX),
                        node_text: hit
                            .node
                            .utf8_text(text.as_bytes())
                            .unwrap_or_default()
                            .to_string(),
                        kind: hit.node.kind().to_string(),
                        span: span_of(hit.node),
                    });
                }
            }
        }
        Ok(found)
    }

    fn comments_in(&self, file: &Path, text: &str) -> Result<Vec<Comment>> {
        let Some(loaded) = self.grammars.for_path(file)? else {
            return Ok(Vec::new());
        };
        let Some(tree) = loaded.parse(text)? else {
            return Ok(Vec::new());
        };
        Ok(Captures::of(&loaded, &tree, text)
            .nodes(&[COMMENT])
            .into_iter()
            .map(|node| comment_of(node, text))
            .collect())
    }

    fn enclosing(
        &self,
        file: &Path,
        text: &str,
        span: Span,
        kinds: &[String],
    ) -> Result<Option<Span>> {
        let Some(loaded) = self.grammars.for_path(file)? else {
            return Ok(None);
        };
        let Some(tree) = loaded.parse(text)? else {
            return Ok(None);
        };
        let at = tree
            .root_node()
            .descendant_for_byte_range(span.start_byte, span.end_byte);
        let found = std::iter::successors(at, tree_sitter::Node::parent)
            .find(|node| kinds.iter().any(|kind| kind == node.kind()));
        Ok(found.map(span_of))
    }
}

/// Where the node is.
fn span_of(node: tree_sitter::Node) -> Span {
    Span {
        start_row: node.start_position().row,
        end_row: node.end_position().row,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
    }
}

/// A comment the language's queries capture as `@comment.outer`.
fn comment_of(node: tree_sitter::Node, text: &str) -> Comment {
    Comment {
        line: u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX),
        text: node
            .utf8_text(text.as_bytes())
            .unwrap_or_default()
            .to_string(),
    }
}
