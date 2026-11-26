//! `serve`, and the product the host serves: what each editor event means to spellbook.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lsp_server::{Connection, ResponseError};
use lsp_types::ServerCapabilities;
use serde_json::Value;
use sorcerer_lsp::serve::unknown_method;
use sorcerer_lsp::{Client, Product, Reply};

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
            landing: Mutex::new(HashMap::new()),
        });
        SpellbookServer { server }
    }
}

impl Product for SpellbookServer {
    fn capabilities() -> ServerCapabilities {
        commands::capabilities()
    }

    /// The open documents' text and version are kept (full sync), so a region file the editor
    /// has open is synced against what the buffer holds and its edit is tied to the buffer's
    /// version. The marks still come from the disk: a hole's mark goes when the file is saved with
    /// its placeholder replaced, not while the person types.
    fn tracks_text() -> bool {
        true
    }

    fn opened(&mut self, path: &Path) {
        self.server.republish(path);
    }

    fn closed(&mut self, path: &Path) {
        self.server.closed(path);
    }

    fn saved(&mut self, path: &Path) {
        self.server.saved(path);
    }

    /// The requests answered at once; the commands go through [`Product::request_later`].
    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        match method {
            "textDocument/codeAction" => self.server.code_actions(params),
            _ => Err(unknown_method(method)),
        }
    }

    /// A command may wait for the editor (an apply whose changes the editor lands).
    fn request_later(&mut self, method: &str, params: Value, reply: Reply) {
        match method {
            "workspace/executeCommand" => self.server.execute(params, reply),
            _ => reply.send(self.request(method, params)),
        }
    }

    fn shutdown(&mut self) {}
}
