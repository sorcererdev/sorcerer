//! Spellbook: a project's conventions as data. A generator under
//! `.sorcerer/spellbook/generators/` writes every file of a convention, with holes that say what
//! belongs where; `spellbook holes` finds the holes still open.
//!
//! `usecases/` is the API (`list_generators`, `choices_for`, `plan_generation`,
//! `apply_plan`, `open_holes`). `domain/` is data and rules with no I/O; `ports/` the traits
//! a use case talks through; `adapters/` implement them (the generator directories, the grammars,
//! the disk, the records); `cli/` parses, calls one use case, prints.

pub mod adapters;
pub mod cli;
pub mod domain;
pub mod ports;
pub mod usecases;
