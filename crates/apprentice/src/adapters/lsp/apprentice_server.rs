//! `serve`, and the product the host serves: what each editor event means to apprentice.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_server::{Connection, ResponseError};
use lsp_types::ServerCapabilities;
use serde_json::Value;
use sorcerer_lsp::serve::unknown_method;
use sorcerer_lsp::{Client, Product};

use crate::adapters::jsonl_task_store::JsonlTaskStore;
use crate::adapters::notes_dir::NotesDir;
use crate::adapters::state_files::StateFiles;
use crate::domain::task::TaskState;
use crate::ports::task_store::TaskStore;
use crate::usecases::end_task::record_task_end;

use super::commands;
use super::server_config::ServerConfig;
use super::server_state::Server;

/// Serves apprentice over a connection until the editor says `exit`.
pub fn serve(connection: Connection, cfg: ServerConfig) -> anyhow::Result<()> {
    sorcerer_lsp::serve(connection, move |client, root| {
        ApprenticeServer::start(cfg, client, root)
    })
}

struct ApprenticeServer {
    server: Arc<Server>,
}

impl ApprenticeServer {
    /// The server for the root the editor sent (the working directory when it sent none). A
    /// task a server that died left running is recorded `interrupted`, and notes kept in their
    /// old place are moved to `.sorcerer/notes/`.
    fn start(cfg: ServerConfig, client: Client, root: Option<PathBuf>) -> ApprenticeServer {
        let root = root.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let server = Arc::new(Server::new(cfg, client, root));
        server.interrupt_what_was_left_running();
        server.move_old_notes();
        ApprenticeServer { server }
    }
}

impl Server {
    fn move_old_notes(&self) {
        let Some(project) = self.project() else {
            return;
        };
        if let Ok(state) = StateFiles::for_project(&project) {
            NotesDir::for_project(&project).move_from(&state.old_notes());
        }
    }

    fn interrupt_what_was_left_running(&self) {
        let Some(project) = self.project() else {
            return;
        };
        let Ok(state) = StateFiles::for_project(&project) else {
            return;
        };
        let store = JsonlTaskStore::at(state.tasks());
        let Ok(rows) = store.read() else { return };
        for task in rows.tasks.iter().filter(|task| !task.state.is_over()) {
            record_task_end(
                task,
                TaskState::Interrupted,
                &store,
                self.cfg.clock.as_ref(),
            );
        }
    }
}

impl Product for ApprenticeServer {
    fn capabilities() -> ServerCapabilities {
        commands::capabilities()
    }

    fn tracks_text() -> bool {
        true
    }

    fn opened(&mut self, path: &Path) {
        self.server.look_from_open(path);
        self.server.publish(path);
    }

    fn changed(&mut self, path: &Path, _version: i32, _text: &str) {
        self.server.typed(path);
    }

    fn closed(&mut self, path: &Path) {
        self.server.client.publish(path, Vec::new());
    }

    fn saved(&mut self, path: &Path) {
        self.server.keep_on_save(path);
        self.server.saved_file(path);
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        match method {
            "textDocument/codeAction" => self.server.code_actions(&params),
            "workspace/executeCommand" => self.server.execute(&params),
            _ => Err(unknown_method(method)),
        }
    }

    fn shutdown(&mut self) {
        if let Some(task) = self.server.running_task() {
            self.server.stop_task(&task);
        }
    }
}
