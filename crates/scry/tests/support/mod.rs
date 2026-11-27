//! Helpers and doubles shared by scry's integration tests: git fixtures (as
//! core's), a project on the vendored grammars, and stand-ins for the review
//! call, the decider and the clock.
#![allow(dead_code)]
#![allow(unused_imports)]

mod doubles;
mod fixed_clock;
mod git_fixtures;
mod project_fixtures;
mod store_fixtures;
mod stub_decider;
mod stub_reviewer;
#[path = "../../../sorcerer-core/tests/support/test_grammars.rs"]
mod test_grammars;

pub use doubles::*;
pub use fixed_clock::*;
pub use git_fixtures::*;
pub use project_fixtures::*;
pub use store_fixtures::*;
pub use stub_decider::*;
pub use stub_reviewer::*;
#[allow(unused_imports)]
pub use test_grammars::*;
