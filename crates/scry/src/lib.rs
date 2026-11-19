//! Scry: reads a change and says what it gets wrong. Config, kinds, steering
//! and the finding store live here; the pass and the CLI come in later modules.

pub mod config;
pub mod find;
pub mod finding;
pub mod judge;
pub mod kind;
pub mod scope;
pub mod steering;
pub mod store;
