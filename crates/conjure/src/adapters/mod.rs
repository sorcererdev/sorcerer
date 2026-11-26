//! What implements the ports: the project's files, the models, the records.

// spellbook:region adapters-modules (from crates/conjure/src/adapters/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod editor_view;
pub mod imports;
pub mod jsonl_change_store;
pub mod project_context;
pub mod provider_writer;
pub mod research_writer;
pub mod scry_files;
pub mod steering_files;
pub mod system_clock;
pub mod why_text;
pub mod wiring;
// spellbook:end adapters-modules
// spellbook:region adapters-folders (from crates/conjure/src/adapters/*/mod.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod lsp;
// spellbook:end adapters-folders
