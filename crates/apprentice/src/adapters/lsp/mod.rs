//! apprentice's language server: regions cut from the person's edits and triaged on idle,
//! answered and checked on save, offered as tasks; tasks that speak through the editor; their
//! marks and what the person does with them.
//!
//! - `apprentice_server.rs`: `serve` and the product the host serves; `server_config.rs` and
//!   `server_state.rs`: what it is built from and what its threads share.
//! - `watch.rs`: the last looks, the idle clock, the save.
//! - `regions.rs`: triage, answers, and a region's actions.
//! - `tasks.rs`: a task from the editor, its end, its marks and their actions.
//! - `editor_port.rs`: `LspEditor`, the `Editor` port over the protocol (`write_file` as
//!   `workspace/applyEdit`, `ghost`, `note`, `progress`, the person's edits since a look).
//! - `asks.rs`: the agent's questions waiting on a line or in the picker.
//! - `publisher.rs`: a file's marks; `commands.rs`: the capabilities, code actions, commands.

// spellbook:region lsp-modules (from crates/apprentice/src/adapters/lsp/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
mod apprentice_server;
mod asks;
mod commands;
mod editor_port;
mod publisher;
mod regions;
mod server_config;
mod server_state;
mod tasks;
mod watch;
// spellbook:end lsp-modules

pub use apprentice_server::serve;
pub use server_config::{IDLE, OpenProject, ServerConfig, WireDecider, WireResearcher};
