//! The traits scry's use cases talk to the world through (the store, the
//! reviewer, the clock, ...), so tests and the eval can stand in for it.

pub mod claim_checker;
pub mod clock;
pub mod finding_store;
pub mod reviewer;
pub mod run_log;
pub mod settings;
