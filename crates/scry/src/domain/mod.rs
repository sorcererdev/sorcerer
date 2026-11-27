//! Data and rules: no I/O, nothing from `ports`, `usecases`, `adapters` or
//! `cli`.

// spellbook:region domain-modules (from crates/scry/src/domain/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod budget;
pub mod config;
pub mod conjure_drops;
pub mod finding;
pub mod findings;
pub mod issue_type;
pub mod judging;
pub mod log_text;
pub mod placing;
pub mod review_prompt;
pub mod scope;
pub mod steering;
pub mod verification;
// spellbook:end domain-modules
