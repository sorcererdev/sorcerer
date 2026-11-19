//! Scry: reads a change and says what it gets wrong. Config, kinds, steering
//! and the finding store live here; the pass is `pass`; the CLI comes in a later module.

pub mod config;
pub mod find;
pub mod finding;
pub mod judge;
pub mod kind;
pub mod pass;
pub mod rounds;
pub mod scope;
pub mod steering;
pub mod store;
