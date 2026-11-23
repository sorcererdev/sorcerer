//! `serve`, and the product the host serves: what each editor event means to conjure.

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

/// Serves conjure over a connection until the editor says `exit`.
pub fn serve(connection: Connection, cfg: ServerConfig) -> anyhow::Result<()> {
    sorcerer_lsp::serve(connection, move |client, root| {
        ConjureServer::start(cfg, client, root)
    })
}

/// The product the host serves: a handle on the server's shared state.
struct ConjureServer {
    pub(super) server: Arc<Server>,
}

impl ConjureServer {
    /// The server for the root the editor sent (the working directory when it sent none).
    pub(super) fn start(cfg: ServerConfig, client: Client, root: Option<PathBuf>) -> ConjureServer {
        let server = Arc::new(Server {
            cfg,
            client,
            root: root.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            published: Mutex::new(HashMap::new()),
            shown: Mutex::new(None),
            running: Mutex::new(None),
        });
        ConjureServer { server }
    }
}

impl Product for ConjureServer {
    fn capabilities() -> ServerCapabilities {
        commands::capabilities()
    }

    fn tracks_text() -> bool {
        true
    }

    fn opened(&mut self, path: &Path) {
        self.server.say_once_if_it_cannot_edit();
        self.server.republish(path);
    }

    fn changed(&mut self, path: &Path, _version: i32, _text: &str) {
        self.server.changed(path);
    }

    fn closed(&mut self, path: &Path) {
        self.server.closed(path);
    }

    fn saved(&mut self, path: &Path) {
        self.server.saved(path);
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        match method {
            "textDocument/codeAction" => self.server.code_actions(params),
            "workspace/executeCommand" => self.server.execute(params),
            _ => Err(unknown_method(method)),
        }
    }

    fn shutdown(&mut self) {
        self.server.shutdown();
    }
}
