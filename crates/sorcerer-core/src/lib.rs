//! Sorcerer's core: the project and its config, languages and grammars,
//! units and fingerprints, facts, stores, the ledger, cost and fences.
//! Synchronous, plain Rust, and no model calls: those live in
//! `sorcerer-models`.
//!
//! `domain/` is data and rules with no I/O; `ports/` are the traits a
//! product talks through (the codebase view, the ledger, the environment);
//! `adapters/` read the disk, git and the network behind them.

pub mod adapters;
pub mod cancel;
pub mod domain;
pub mod error;
pub mod ports;

pub use cancel::Cancel;
pub use error::{Error, Result};
