//! Sorcerer's core: the project and its config, languages and grammars,
//! units and fingerprints, facts, stores, the ledger, cost and fences.
//! Synchronous, plain Rust, and no model calls: those live in
//! `sorcerer-models`.

pub mod cancel;
pub mod config;
pub mod env;
pub mod error;

pub use cancel::Cancel;
pub use error::{Error, Result};
