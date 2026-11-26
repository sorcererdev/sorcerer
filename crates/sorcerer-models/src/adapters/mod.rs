//! The providers, Jev, the researchers, and their doubles.

// spellbook:region adapters-modules (from crates/sorcerer-models/src/adapters/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod anthropic;
pub mod claude;
pub mod claude_code;
pub mod http;
pub mod jev;
pub mod openai;
pub mod providers;
pub mod recorded_decider;
pub mod recorded_researcher;
pub mod researchers;
pub mod scripted_provider;
pub mod tool_loop;
pub mod tools;
// spellbook:end adapters-modules
