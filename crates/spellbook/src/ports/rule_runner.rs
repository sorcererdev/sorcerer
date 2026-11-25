//! Running rules over a file's text. Behind a port so the use cases do not touch a parser.

use std::path::Path;

use sorcerer_core::Result;

use crate::domain::allow::Comment;
use crate::domain::finding::{RawFinding, Span};
use crate::domain::rule::Rule;

pub trait RuleRunner: Send + Sync {
    /// What the rules find in the file's text: one raw finding for each place a rule's match or
    /// `@finding` capture starts. The file (relative to the root) says which language to parse
    /// it in. An error names a rule that cannot run (a query with no `@finding`, or one the
    /// file's language does not accept).
    fn findings_in(&self, file: &Path, text: &str, rules: &[&Rule]) -> Result<Vec<RawFinding>>;

    /// The comments the parser finds in the file's text, none when the file's language is not
    /// known or the text cannot be parsed.
    fn comments_in(&self, file: &Path, text: &str) -> Result<Vec<Comment>>;

    /// The nearest node around `span` (the node at the span itself counts) whose kind is one of
    /// `kinds`; `None` when there is none, or the file's language is not known.
    ///
    /// Example: for the span of `sqlite3.connect` in `conn = sqlite3.connect(path)`, the kinds
    /// `["expression_statement"]` give the whole statement's span.
    fn enclosing(
        &self,
        file: &Path,
        text: &str,
        span: Span,
        kinds: &[String],
    ) -> Result<Option<Span>>;
}
