//! Sorcerer's core: the project and its config, languages and grammars,
//! units and fingerprints, facts, stores, the ledger, cost and fences.
//! Synchronous, plain Rust, and no model calls: those live in
//! `sorcerer-models`.

pub mod cancel;
pub mod compare;
pub mod config;
pub mod env;
pub mod error;
pub mod facts;
pub mod fingerprint;
pub mod fnv;
pub mod grammar;
pub mod language;
pub mod project;
pub mod query;
pub mod unit;
pub mod view;

pub use cancel::Cancel;
pub use error::{Error, Result};
pub use project::Project;
