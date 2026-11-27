//! The disk, git, the environment and the network behind the ports, and
//! the I/O half of domain types (`Project::open`, `Fences::for_project`,
//! `Languages::load`).

// spellbook:region adapters-modules (from crates/sorcerer-core/src/adapters/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod env;
pub mod fenced_links_cache;
pub mod fences;
pub mod files_view;
pub mod grammar;
pub mod json_store;
pub mod jsonl_ledger;
pub mod project;
pub mod trust_list;
// spellbook:end adapters-modules
// spellbook:region adapters-folders (from crates/sorcerer-core/src/adapters/*/mod.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod platform;
// spellbook:end adapters-folders
