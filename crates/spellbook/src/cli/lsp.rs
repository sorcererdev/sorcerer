//! `spellbook lsp`: the editor's language server over standard input and output.

use anyhow::Result;
use lsp_server::Connection;
use sorcerer_core::ports::env::Env;

use crate::adapters::lsp::{ServerConfig, serve};

/// Serves the editor until it says `exit`, then exits 0. Nothing else may write to standard
/// output while it runs: the stream is the protocol's.
pub(super) fn lsp(env: &dyn Env) -> Result<i32> {
    let (connection, io_threads) = Connection::stdio();
    serve(connection, ServerConfig::live(env))?;
    io_threads.join()?;
    Ok(0)
}
