//! Running rules over a file's text. Behind a port so the use cases do not touch a parser.

use std::path::Path;

use sorcerer_core::Result;

use crate::domain::finding::RawFinding;
use crate::domain::rule::Rule;

pub trait RuleRunner: Send + Sync {
    /// What the rules find in the file's text: one raw finding for each place a rule's match or
    /// `@finding` capture starts. The file (relative to the root) says which language to parse
    /// it in. An error names a rule that cannot run (a query with no `@finding`, or one the
    /// file's language does not accept).
    fn findings_in(&self, file: &Path, text: &str, rules: &[&Rule]) -> Result<Vec<RawFinding>>;
}
