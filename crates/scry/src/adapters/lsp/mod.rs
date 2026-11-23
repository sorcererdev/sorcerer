//! scry's language server: findings as diagnostics in the editor.
//!
//! The host (`sorcerer-lsp`) does the protocol; this module is what a save means to scry. It
//! serves one project, the one found from the root the editor sends at initialize. A file
//! belongs to it when the nearest project root above the file is that root; the server
//! ignores every other file, and every file the project has no language for.
//!
//! What the editor shows is what `scry` would print: `shown_in_file`, read afresh from the
//! store each time, for open documents only. Publishing runs on one thread of its own, in the
//! order it was asked for, so the main loop never waits on a parse or a grammar build and a
//! close always lands after the open before it.

mod commands;
mod diagnostics;
mod passes;
mod publisher;
mod scry_server;
mod server_config;
mod server_state;
mod trust;
pub use diagnostics::{STALE_LINE, diagnostic};
use publisher::project_file;
pub use scry_server::serve;
pub use server_config::{OpenProject, SETTLE, ServerConfig, WirePass};
use server_state::{Publish, Server, lock};
