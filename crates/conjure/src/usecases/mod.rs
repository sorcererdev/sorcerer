//! The API: what conjure does, one module a feature.

// spellbook:region usecases-modules (from crates/conjure/src/usecases/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod report_cost;
pub mod show_log;
// spellbook:end usecases-modules
// spellbook:region usecases-folders (from crates/conjure/src/usecases/*/mod.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod conjure_change;
// spellbook:end usecases-folders
