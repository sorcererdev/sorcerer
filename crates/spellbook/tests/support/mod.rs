//! Helpers shared by spellbook's integration tests: a project on the vendored grammars, and a
//! copy of bistro holding a `command` generator.
#![allow(dead_code)]

mod bistro_fixtures;
mod project_fixtures;

#[allow(unused_imports)]
pub use bistro_fixtures::*;
#[allow(unused_imports)]
pub use project_fixtures::*;
