//! Helpers shared by core's integration tests.
#![allow(dead_code)]

mod git_fixtures;
mod grammar_fixtures;
mod language_corpora;
mod test_grammars;

#[allow(unused_imports)]
pub use git_fixtures::*;
#[allow(unused_imports)]
pub use grammar_fixtures::*;
#[allow(unused_imports)]
pub use language_corpora::*;
#[allow(unused_imports)]
pub use test_grammars::*;
