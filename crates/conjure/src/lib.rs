//! Conjure: writes one small change where you ask.
//!
//! `usecases/` is the API (`conjure_change`, `report_cost`, `show_log`). `domain/` is data and
//! rules with no I/O; `ports/` the traits a use case talks through; `adapters/` implement them
//! (the models, the project's files, the records, scry's files, the wiring); `cli/` parses,
//! calls one use case, prints.

pub mod adapters;
pub mod cli;
pub mod domain;
pub mod ports;
pub mod usecases;
