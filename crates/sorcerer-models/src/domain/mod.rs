//! Data and rules: no I/O, nothing from `ports`, `usecases` or `adapters`.

// spellbook:region domain-modules (from crates/sorcerer-models/src/domain/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod claim;
pub mod claim_prompt;
pub mod decide;
pub mod model;
pub mod research;
// spellbook:end domain-modules
