//! spellbook's language server: four commands the editor's pickers drive (the generators, the
//! choices of an input, a plan, an apply), and the hole marks it shows.
//!
//! The host (`sorcerer-lsp`) does the protocol; this module is what spellbook does with it. It
//! serves one project, the one found from the root the editor sends at initialize.
//!
//! - `spellbook_server.rs`: `serve` and the product the host serves; `server_config.rs` and
//!   `server_state.rs`: what it is built from and what it shares.
//! - `commands.rs`: the capabilities and the four commands.
//! - `plans.rs`: the plans made and not yet applied.
//! - `marks.rs` and `diagnostics.rs`: a hole as a diagnostic, and the diagnostics of one file.
//! - `project_paths.rs`: which editor paths are the project's.

mod commands;
mod diagnostics;
mod marks;
mod plans;
mod project_paths;
mod server_config;
mod server_state;
mod spellbook_server;

pub use server_config::{OpenProject, ServerConfig};
use server_state::{NOT_TRUSTED, Server, lock};
pub use spellbook_server::serve;
