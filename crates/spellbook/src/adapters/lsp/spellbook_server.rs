//! `serve`, and the product the host serves: what each editor event means to spellbook.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lsp_server::{Connection, ResponseError};
use lsp_types::ServerCapabilities;
use serde_json::Value;
use sorcerer_lsp::serve::unknown_method;
use sorcerer_lsp::{Client, Product};

use super::commands;
use super::server_config::ServerConfig;
use super::server_state::Server;

/// Serves spellbook over a connection until the editor says `exit`.
pub fn serve(connection: Connection, cfg: ServerConfig) -> anyhow::Result<()> {
    sorcerer_lsp::serve(connection, move |client, root| {
        SpellbookServer::start(cfg, client, root)
    })
}

/// The product the host serves: a handle on the server's shared state.
struct SpellbookServer {
    server: Arc<Server>,
}

impl SpellbookServer {
    /// The server for the root the editor sent (the working directory when it sent none).
    fn start(cfg: ServerConfig, client: Client, root: Option<PathBuf>) -> SpellbookServer {
        let server = Arc::new(Server {
            cfg,
            client,
            root: root.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            published: Mutex::new(HashMap::new()),
            plans: Mutex::new(Default::default()),
        });
        SpellbookServer { server }
    }
}

impl Product for SpellbookServer {
    fn capabilities() -> ServerCapabilities {
        commands::capabilities()
    }

    /// The marks come from the disk: a hole's mark goes when the file is saved with its
    /// placeholder replaced, not while the person types.
    fn tracks_text() -> bool {
        false
    }

    fn opened(&mut self, _path: &Path) {}

    fn closed(&mut self, _path: &Path) {}

    fn saved(&mut self, _path: &Path) {}

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        match method {
            "workspace/executeCommand" => self.server.execute(params),
            _ => Err(unknown_method(method)),
        }
    }

    fn shutdown(&mut self) {}
}
