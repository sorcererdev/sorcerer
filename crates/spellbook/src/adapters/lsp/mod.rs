//! spellbook's language server: four commands the editor's pickers drive (the generators, the
//! choices of an input, a plan, an apply), the hole marks and findings it shows, and a finding's
//! fixes as quick fixes.
//!
//! The host (`sorcerer-lsp`) does the protocol; this module is what spellbook does with it. It
//! serves one project, the one found from the root the editor sends at initialize.
//!
//! - `spellbook_server.rs`: `serve` and the product the host serves; `server_config.rs` and
//!   `server_state.rs`: what it is built from and what it shares.
//! - `commands.rs`: the capabilities and the four commands.
//! - `code_actions.rs`: a finding's fixes as quick fixes.
//! - `plans.rs`: the plans made and not yet applied.
//! - `landing.rs`: a plan's changed files landed by the editor as one undoable edit.
//! - `sync_on_save.rs`: the owned regions synced after a save, in the editor or on disk.
//! - `marks.rs`, `findings.rs` and `diagnostics.rs`: a hole and a finding as diagnostics, and the
//!   diagnostics of one file (the marks and the findings published together).
//! - `project_paths.rs`: which editor paths are the project's.

// spellbook:region lsp-modules (from crates/spellbook/src/adapters/lsp/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
mod code_actions;
mod commands;
mod diagnostics;
mod findings;
mod landing;
mod marks;
mod plans;
mod project_paths;
mod server_config;
mod server_state;
mod spellbook_server;
mod sync_on_save;
// spellbook:end lsp-modules

pub use server_config::{OpenProject, ServerConfig};
use server_state::{NOT_TRUSTED, Server, lock};
pub use spellbook_server::serve;
