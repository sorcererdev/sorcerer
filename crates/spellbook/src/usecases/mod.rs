//! The API: each use case is one thing spellbook does, given its ports.

// spellbook:region usecases-modules (from crates/spellbook/src/usecases/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod apply_plan;
pub mod check_files;
pub mod check_project;
pub mod check_since;
pub mod choices_for;
pub mod fixes_for;
pub mod list_generators;
pub mod open_holes;
pub mod plan_fix;
pub mod plan_generation;
pub mod planned_view;
pub mod sync_regions;
// spellbook:end usecases-modules
