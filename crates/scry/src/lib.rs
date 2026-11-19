//! Scry: reads a change and says what it gets wrong.
//!
//! `usecases/` is the API, one module a feature (`review_change`, its steps
//! private; `list_findings`, `explain_finding`, `dismiss_finding`,
//! `report_cost`, `check_setup`). `domain/` is data and rules with no I/O;
//! `ports/` the traits a use case talks through; `adapters/` implement them
//! (the store file, the run log, `.sorcerer/` settings, the reviewer over a
//! researcher, the wiring); `cli/` parses, calls one use case, prints.

pub mod adapters;
pub mod cli;
pub mod domain;
pub mod ports;
pub mod usecases;
