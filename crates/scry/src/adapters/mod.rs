//! The real things behind the ports: the findings store as a JSON file, the
//! run log as Markdown files, the settings read from `.sorcerer/`, the
//! reviewer as a research call, the system clock, and the wiring that
//! builds them all for a project; and the editor's language server, which
//! calls the same use cases the command line calls.

// spellbook:region adapters-modules (from crates/scry/src/adapters/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod json_finding_store;
pub mod markdown_run_log;
pub mod research_reviewer;
pub mod scry_toml;
pub mod system_clock;
pub mod why_text;
pub mod wiring;
// spellbook:end adapters-modules
// spellbook:region adapters-folders (from crates/scry/src/adapters/*/mod.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod lsp;
// spellbook:end adapters-folders
