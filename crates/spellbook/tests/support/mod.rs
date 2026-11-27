//! Helpers shared by spellbook's integration tests: a project on the vendored grammars, and a
//! copy of bistro holding a `command` generator.
#![allow(dead_code)]

mod bistro_fixtures;
mod corpus_cases;
mod fix_fixtures;
mod fixed_clock;
#[path = "../../../sorcerer-core/tests/support/language_corpora.rs"]
mod language_corpora;
mod plan_rig;
mod project_fixtures;
#[path = "../../../sorcerer-core/tests/support/test_grammars.rs"]
mod test_grammars;

#[allow(unused_imports)]
pub use bistro_fixtures::*;
#[allow(unused_imports)]
pub use corpus_cases::*;
#[allow(unused_imports)]
pub use fix_fixtures::*;
#[allow(unused_imports)]
pub use fixed_clock::*;
#[allow(unused_imports)]
pub use language_corpora::*;
#[allow(unused_imports)]
pub use plan_rig::*;
#[allow(unused_imports)]
pub use project_fixtures::*;
#[allow(unused_imports)]
pub use test_grammars::*;
