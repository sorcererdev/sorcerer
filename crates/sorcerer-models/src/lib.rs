//! Sorcerer's models: providers, the researcher, the decider (Jev) and the
//! claim checker. Everything here can spend money, so every paid call is
//! recorded in the ledger, and each piece has a recorded double for tests.

pub mod error;
pub mod provider;

pub use error::{Error, Result};
