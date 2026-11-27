//! `serve`, and the product the host serves: what each editor event means to scry.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use lsp_server::{Connection, ResponseError};
use lsp_types::ServerCapabilities;
use serde_json::Value;
use sorcerer_lsp::line_edits::same_text;
use sorcerer_lsp::serve::unknown_method;
use sorcerer_lsp::{Client, Jobs, Product};

use super::commands;
use super::passes;
use super::publisher::start_publisher;
use super::server_config::ServerConfig;
use super::server_state::{Publish, Server};
use super::trust;
use crate::adapters::wiring::Wiring;

/// Serves scry over a connection until the editor says `exit`.
pub fn serve<W: Wiring + 'static>(
    connection: Connection,
    cfg: ServerConfig<W>,
) -> anyhow::Result<()> {
    sorcerer_lsp::serve(connection, move |client, root| {
        ScryServer::start(cfg, client, root)
    })
}

/// The product the host serves: a handle on the server's shared state.
struct ScryServer<W: Wiring> {
    pub(super) server: Arc<Server<W>>,
}

impl<W: Wiring + 'static> ScryServer<W> {
    /// The server for the root the editor sent (the working directory when it sent none),
    /// with its publisher thread started.
    pub(super) fn start(
        cfg: ServerConfig<W>,
        client: Client,
        root: Option<PathBuf>,
    ) -> ScryServer<W> {
        let (publisher, to_publish) = channel();
        let server = Arc::new(Server {
            cfg,
            client,
            root: root.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            publisher: Mutex::new(publisher),
            errors_logged: Mutex::new(BTreeSet::new()),
            published: Mutex::new(HashMap::new()),
            jobs: Jobs::new(),
            passes: Mutex::new(passes::Passes::default()),
            trust_question: Mutex::new(trust::TrustQuestion::default()),
        });
        start_publisher(Arc::downgrade(&server), to_publish);
        ScryServer { server }
    }
}

impl<W: Wiring + 'static> Product for ScryServer<W> {
    fn capabilities() -> ServerCapabilities {
        commands::capabilities()
    }

    /// The text is tracked only to see a buffer come back to the file on disk (below).
    fn tracks_text() -> bool {
        true
    }

    fn opened(&mut self, path: &Path) {
        self.server.publish(Publish::File(path.to_path_buf()));
    }

    /// A change that leaves the buffer as the file on disk is shown again: the editor reloaded
    /// a file put back by git (Neovim says so with a change, no open or save), or the person
    /// undid back to the saved text. What scry shows is placed on the saved text, so typing
    /// that leaves the buffer unlike the disk publishes nothing and waits for the save.
    fn changed(&mut self, path: &Path, _version: i32, text: &str) {
        let on_disk = std::fs::read_to_string(path).unwrap_or_default();
        if same_text(text, &on_disk) {
            self.server.publish(Publish::File(path.to_path_buf()));
        }
    }

    fn closed(&mut self, path: &Path) {
        self.server.publish(Publish::Clear(path.to_path_buf()));
    }

    fn saved(&mut self, path: &Path) {
        self.server.publish(Publish::File(path.to_path_buf()));
        self.server.saved(path.to_path_buf());
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
