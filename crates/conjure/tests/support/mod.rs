//! Helpers shared by conjure's integration tests: git fixtures, a project on the vendored
//! grammars, and doubles for the writer and the clock.
#![allow(dead_code)]

mod change_rig;
mod deciders;
mod fixed_clock;
mod git_fixtures;
#[path = "../../../sorcerer-core/tests/support/language_corpora.rs"]
mod language_corpora;
mod project_fixtures;
mod rule_checkers;
#[path = "../../../sorcerer-core/tests/support/test_grammars.rs"]
mod test_grammars;

#[allow(unused_imports)]
pub use change_rig::*;
#[allow(unused_imports)]
pub use deciders::*;
#[allow(unused_imports)]
pub use fixed_clock::*;
#[allow(unused_imports)]
pub use git_fixtures::*;
#[allow(unused_imports)]
pub use language_corpora::*;
#[allow(unused_imports)]
pub use project_fixtures::*;
#[allow(unused_imports)]
pub use rule_checkers::*;
#[allow(unused_imports)]
pub use test_grammars::*;
