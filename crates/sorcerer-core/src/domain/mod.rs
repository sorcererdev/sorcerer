//! Data and rules: no I/O, nothing from `ports` or `adapters`.

// spellbook:region domain-modules (from crates/sorcerer-core/src/domain/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod broken_region;
pub mod captures;
pub mod compare;
pub mod config;
pub mod cost;
pub mod errand;
pub mod facts;
pub mod fences;
pub mod fingerprint;
pub mod fnv;
pub mod generated_regions;
pub mod language;
pub mod name_forms;
pub mod paths;
pub mod project;
pub mod query;
pub mod template_blocks;
pub mod template_conditions;
pub mod template_paths;
pub mod templates;
pub mod trust;
pub mod unit;
pub mod versioned;
// spellbook:end domain-modules
