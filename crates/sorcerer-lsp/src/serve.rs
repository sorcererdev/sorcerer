//! The handshake and the main loop: what a product plugs into.

use std::path::{Path, PathBuf};

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response, ResponseError};
use lsp_types::{
    SaveOptions, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions,
};
use serde_json::{Value, json};

use crate::client::{Abilities, Client};
use crate::uri::path_of;

/// A product's language server: what it does when a document opens, closes or is saved, and
/// how it answers a request. Each call is made on the main loop, so it must not wait on
/// anything slow; slow work goes to a thread and speaks through the [`Client`].
pub trait Product {
    /// What the product serves beyond documents (code actions, commands). The host replaces
    /// the text sync with its own: open and close, and save without the text.
    fn capabilities() -> ServerCapabilities
    where
        Self: Sized;
    /// The editor opened a file.
    fn opened(&mut self, path: &Path);
    /// The editor closed a file.
    fn closed(&mut self, path: &Path);
    /// The editor saved a file.
    fn saved(&mut self, path: &Path);
    /// A request from the editor, by method name. An unknown method is [`unknown_method`].
    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError>;
    /// The editor asked the server to shut down; nothing more will be asked of it.
    fn shutdown(&mut self);
}

/// The error for a method the product does not serve (`-32601`).
pub fn unknown_method(method: &str) -> ResponseError {
    ResponseError {
        code: ErrorCode::MethodNotFound as i32,
        message: format!("unknown method {method}"),
        data: None,
    }
}

/// Serves a product over a connection until the editor says `exit` or hangs up.
///
/// 1. The handshake: the editor's abilities and its workspace root are read, and the
///    product's capabilities are announced with text sync forced to open, close and save.
/// 2. `make` builds the product with the client and the root (the first workspace folder,
///    else `rootUri`; `None` when the editor sent neither).
/// 3. The loop: opens, closes and saves of `file:` documents go to the product (other URIs are
///    dropped), requests are answered by it, replies go to whatever waits for them, and
///    `shutdown` is passed on and answered.
pub fn serve<P: Product>(
    connection: Connection,
    make: impl FnOnce(Client, Option<PathBuf>) -> P,
) -> anyhow::Result<()> {
    let (client, root) = handshake(&connection, &P::capabilities())?;
    let mut product = make(client.clone(), root);
    for message in &connection.receiver {
        match message {
            Message::Request(request) => answer(&connection, &mut product, request),
            Message::Response(response) => client.replied(response),
            Message::Notification(note) if note.method == "exit" => return Ok(()),
            Message::Notification(note) => pass_on(&client, &mut product, &note),
        }
    }
    Ok(())
}

/// Answers `initialize` and waits for `initialized`.
fn handshake(
    connection: &Connection,
    capabilities: &ServerCapabilities,
) -> anyhow::Result<(Client, Option<PathBuf>)> {
    let (id, params) = connection.initialize_start()?;
    let announced = ServerCapabilities {
        text_document_sync: Some(open_close_and_save()),
        ..capabilities.clone()
    };
    connection.initialize_finish(id, json!({ "capabilities": announced }))?;
    let client = Client::new(connection.sender.clone(), abilities_of(&params));
    Ok((client, workspace_root(&params)))
}

/// Opens, closes and saves (without the text), and no change while the person types.
fn open_close_and_save() -> TextDocumentSyncCapability {
    TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
        open_close: Some(true),
        change: None,
        will_save: None,
        will_save_wait_until: None,
        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
            include_text: Some(false),
        })),
    })
}

fn abilities_of(params: &Value) -> Abilities {
    let says = |pointer: &str| params.pointer(pointer).and_then(Value::as_bool) == Some(true);
    Abilities {
        work_done_progress: says("/capabilities/window/workDoneProgress"),
        related_information: says(
            "/capabilities/textDocument/publishDiagnostics/relatedInformation",
        ),
    }
}

/// The first workspace folder, else `rootUri`.
fn workspace_root(params: &Value) -> Option<PathBuf> {
    let uri = params
        .pointer("/workspaceFolders/0/uri")
        .or_else(|| params.pointer("/rootUri"))
        .and_then(Value::as_str)?;
    path_of(&uri.parse().ok()?)
}

/// A request answered by the product; `shutdown` is passed on and answered with null.
fn answer(connection: &Connection, product: &mut impl Product, request: Request) {
    let Request { id, method, params } = request;
    let response = if method == "shutdown" {
        product.shutdown();
        Response::new_ok(id, Value::Null)
    } else {
        Response {
            id,
            response_result: product.request(&method, params),
        }
    };
    let _ = connection.sender.send(Message::Response(response));
}

/// An open, a close or a save of a file, passed to the product; the host keeps the open set
/// first, so the product sees it already changed.
fn pass_on(client: &Client, product: &mut impl Product, note: &Notification) {
    let Some(path) = document_path(note) else {
        return;
    };
    match note.method.as_str() {
        "textDocument/didOpen" => {
            client.opened(&path);
            product.opened(&path);
        }
        "textDocument/didClose" => {
            client.closed(&path);
            product.closed(&path);
        }
        "textDocument/didSave" => product.saved(&path),
        _ => {}
    }
}

/// The file a document notification is about; `None` for any other notification or a URI
/// that is not a file.
fn document_path(note: &Notification) -> Option<PathBuf> {
    let uri = note.params.pointer("/textDocument/uri")?.as_str()?;
    path_of(&uri.parse().ok()?)
}
