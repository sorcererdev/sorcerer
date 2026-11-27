//! The service layer: what lackey does, one module a use case. The command line and the
//! editor's server call these.

pub mod run_task;
// spellbook:region usecases-modules (from crates/lackey/src/usecases/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod answer_regions;
pub mod end_task;
pub mod label_region;
pub mod offer_task;
pub mod report_cost;
pub mod show_log;
pub mod triage_regions;
// spellbook:end usecases-modules
