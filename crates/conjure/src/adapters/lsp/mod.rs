//! conjure's language server: the code actions that conjure a change, the change landed in the
//! editor through one `workspace/applyEdit`, its marks, and what the person does with it.
//!
//! The host (`sorcerer-lsp`) does the protocol and keeps the open documents' text; this module
//! is what conjure does with them. It serves one project, the one found from the root the
//! editor sends at initialize, and ignores every file that is not the project's.
//!
//! - `conjure_server.rs`: `serve` and the product the host serves; `server_config.rs` and
//!   `server_state.rs`: what it is built from and what its threads share; `project_paths.rs`:
//!   which editor paths are the project's.
//! - `commands.rs`: the capabilities, the code actions on a line and the commands they run.
//! - `changes.rs`: conjuring a change on a thread, landing it, and the change's life after:
//!   Undo, Try harder, Show why, Show <file>, and the choice a save, an undo or the next change
//!   records.
//! - `landing.rs`: a change as one `WorkspaceEdit`, and its undo.
//! - `undo.rs`: Undo, and Try harder, which undoes and conjures again.
//! - `marks.rs`: the marks (diagnostics) a landed change shows and their words.
//!
//! One change is marked at a time (`shown`), and one is conjured at a time (`running`).

pub(crate) mod changes;
pub(crate) mod commands;
mod conjure_server;
mod landing;
mod marks;
mod project_paths;
mod server_config;
mod server_state;
mod undo;

pub use conjure_server::serve;
use project_paths::{project_file, project_relative};
pub use server_config::{OpenProject, ServerConfig, WireWriter};
use server_state::{NOT_TRUSTED, Server, lock};
