//! Data and the rules over it, with no I/O.

// spellbook:region domain-modules (from crates/apprentice/src/domain/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod agent_event;
pub mod answer_prompt;
pub mod budget;
pub mod config;
pub mod cutting;
pub mod edits_text;
pub mod estimate;
pub mod log_text;
pub mod loopback_message;
pub mod mediums;
pub mod paths;
pub mod permissions;
pub mod prompt;
pub mod region;
pub mod region_record;
pub mod rules;
pub mod scry_notes;
pub mod status;
pub mod task;
pub mod token_cost;
pub mod tree_readers;
pub mod triage;
// spellbook:end domain-modules
