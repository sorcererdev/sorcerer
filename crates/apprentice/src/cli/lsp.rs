//! `apprentice lsp`: the editor's language server over standard input and output.

use std::sync::Arc;

use anyhow::Result;
use lsp_server::Connection;
use sorcerer_core::adapters::env::SystemEnv;

use crate::adapters::lsp::{ServerConfig, serve};

/// Serves the editor until it says `exit`, then exits 0. Nothing else may write to standard
/// output while it runs: the stream is the protocol's.
pub(super) fn lsp() -> Result<i32> {
    let (connection, io_threads) = Connection::stdio();
    serve(connection, ServerConfig::live(Arc::new(SystemEnv)))?;
    io_threads.join()?;
    Ok(0)
}
