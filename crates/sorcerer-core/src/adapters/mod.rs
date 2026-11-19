//! The disk, git, the environment and the network behind the ports, and
//! the I/O half of domain types (`Project::open`, `Fences::for_project`,
//! `Languages::load`).

pub mod env;
pub mod fences;
pub mod files_view;
pub mod grammar;
pub mod json_store;
pub mod jsonl_ledger;
pub mod project;
