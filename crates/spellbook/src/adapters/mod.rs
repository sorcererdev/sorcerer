//! The ports implemented: the disk, the grammars, the records.

// spellbook:region adapters-modules (from crates/spellbook/src/adapters/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod baseline_toml;
pub mod cached_grammars;
pub mod checking;
pub mod disk_sorcerer_dir;
pub mod disk_workspace;
pub mod generator_dir;
pub mod git_pack_source;
pub mod json_holes;
pub mod json_report;
pub mod jsonl_generation_store;
pub mod query_anchors;
pub mod query_rules;
pub mod regions_toml;
pub mod rules_toml;
pub mod system_clock;
pub mod wiring;
// spellbook:end adapters-modules
// spellbook:region adapters-folders (from crates/spellbook/src/adapters/*/mod.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod lsp;
// spellbook:end adapters-folders
