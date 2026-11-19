//! Sorcerer's models: providers, the researcher, the decider (Jev) and the
//! claim checker. Everything here can spend money, so every paid call is
//! recorded in the ledger, and each piece has a recorded double for tests.
//!
//! `domain/` is the data of a model call, a question, a research errand and
//! a claim, with no I/O; `ports/` the seams (`Provider`, `Researcher`,
//! `Decider`); `usecases/` the claim check; `adapters/` the providers, Jev,
//! the researchers and their recorded doubles.

pub mod adapters;
pub mod domain;
pub mod error;
pub mod ports;
pub mod usecases;

pub use error::{Error, Result};
