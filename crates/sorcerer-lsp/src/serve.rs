//! The handshake and the main loop: what a product plugs into.

use std::path::{Path, PathBuf};

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response, ResponseError};
use lsp_types::{
    SaveOptions, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions,
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
    /// Whether the product wants the open documents' text. When it does, the editor is asked to
    /// send the whole text on every change, the host keeps each open document's version and
    /// text ([`Client::text_of`]), and [`Product::changed`] is called. Most products do not.
    fn tracks_text() -> bool
    where
        Self: Sized,
    {
        false
    }
    /// The editor opened a file.
    fn opened(&mut self, path: &Path);
    /// The person changed an open file: its new version and whole text. Called only for a
    /// product that tracks text, after the host has kept the text.
    fn changed(&mut self, _path: &Path, _version: i32, _text: &str) {}
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
///    product's capabilities are announced with text sync forced to open, close and save,
///    plus the whole text on each change for a product that tracks text.
/// 2. `make` builds the product with the client and the root (the first workspace folder,
///    else `rootUri`; `None` when the editor sent neither).
/// 3. The loop: opens, closes and saves of `file:` documents go to the product (other URIs are
///    dropped), and changes too when it tracks text, requests are answered by it, replies go to whatever waits for them, and
///    `shutdown` is passed on and answered.
/// 4. The end: when the editor hung up without `exit`, the product is shut down too, and
///    every question still waiting for a reply is dropped.
pub fn serve<P: Product>(
    connection: Connection,
    make: impl FnOnce(Client, Option<PathBuf>) -> P,
) -> anyhow::Result<()> {
    let tracks_text = if P::tracks_text() {
        TracksText::Yes
    } else {
        TracksText::No
    };
    let (client, root) = handshake(&connection, &P::capabilities(), tracks_text)?;
    let mut product = make(client.clone(), root);
    let mut said_exit = false;
    for message in &connection.receiver {
        match message {
            Message::Request(request) => answer(&connection, &mut product, request),
            Message::Response(response) => client.replied(response),
            Message::Notification(note) if note.method == "exit" => {
                said_exit = true;
                break;
            }
            Message::Notification(note) => pass_on(&client, &mut product, &note),
        }
    }
    if !said_exit {
        // The editor is gone without saying `shutdown`: stop the product's work.
        product.shutdown();
    }
    // Nothing will answer now. A waiting callback may hold a client, and so the connection's
    // sender, which would keep the writer thread (and the process) alive.
    client.forget_waiting();
    Ok(())
}

/// Whether the product keeps the open documents' text ([`Product::tracks_text`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TracksText {
    Yes,
    No,
}

/// Answers `initialize` and waits for `initialized`.
fn handshake(
    connection: &Connection,
    capabilities: &ServerCapabilities,
    tracks_text: TracksText,
) -> anyhow::Result<(Client, Option<PathBuf>)> {
    let (id, params) = connection.initialize_start()?;
    let announced = ServerCapabilities {
        text_document_sync: Some(open_close_and_save(tracks_text)),
        ..capabilities.clone()
    };
    connection.initialize_finish(id, json!({ "capabilities": announced }))?;
    let client = Client::new(
        connection.sender.clone(),
        abilities_of(&params),
        tracks_text,
    );
    Ok((client, workspace_root(&params)))
}

/// Opens, closes and saves (without the text). While the person types, nothing for a product
/// that does not track text; the whole text on each change for one that does.
fn open_close_and_save(tracks_text: TracksText) -> TextDocumentSyncCapability {
    let change = match tracks_text {
        TracksText::Yes => Some(TextDocumentSyncKind::FULL),
        TracksText::No => None,
    };
    TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
        open_close: Some(true),
        change,
        will_save: None,
        will_save_wait_until: None,
        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
            include_text: Some(false),
        })),
    })
}

/// What the editor said it can do, from the `initialize` request's capabilities.
fn abilities_of(params: &Value) -> Abilities {
    let says = |pointer: &str| params.pointer(pointer).and_then(Value::as_bool) == Some(true);
    let operations = params
        .pointer("/capabilities/workspace/workspaceEdit/resourceOperations")
        .and_then(Value::as_array);
    let resource_operation = |operation: &str| {
        operations.is_some_and(|operations| operations.iter().any(|named| named == operation))
    };
    Abilities {
        work_done_progress: says("/capabilities/window/workDoneProgress"),
        related_information: says(
            "/capabilities/textDocument/publishDiagnostics/relatedInformation",
        ),
        apply_edit: says("/capabilities/workspace/applyEdit"),
        // resource operations exist only inside documentChanges, so an editor that announces
        // one takes them (Neovim 0.12 announces those and not documentChanges itself)
        document_changes: says("/capabilities/workspace/workspaceEdit/documentChanges")
            || operations.is_some_and(|operations| !operations.is_empty()),
        create_files: resource_operation("create"),
        delete_files: resource_operation("delete"),
        draws_working: params
            .pointer("/initializationOptions/drawsWorking")
            .and_then(Value::as_bool)
            == Some(true),
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

/// An open, a change, a close or a save of a file, passed to the product; the host keeps the
/// open set and the text first, so the product sees them already changed. A change reaches only
/// a product that tracks text.
fn pass_on(client: &Client, product: &mut impl Product, note: &Notification) {
    let Some(path) = document_path(note) else {
        return;
    };
    match note.method.as_str() {
        "textDocument/didOpen" => {
            client.opened(&path, opened_document(note));
            product.opened(&path);
        }
        "textDocument/didChange" if client.tracks_text() => {
            if let Some((version, text)) = changed_document(note) {
                client.changed(&path, version, &text);
                product.changed(&path, version, &text);
            }
        }
        "textDocument/didClose" => {
            client.closed(&path);
            product.closed(&path);
        }
        "textDocument/didSave" => product.saved(&path),
        _ => {}
    }
}

/// The version and text an open notification carries; `None` when either is missing.
fn opened_document(note: &Notification) -> Option<(i32, String)> {
    let version = note.params.pointer("/textDocument/version")?.as_i64()?;
    let text = note.params.pointer("/textDocument/text")?.as_str()?;
    Some((i32::try_from(version).ok()?, text.to_string()))
}

/// The new version and whole text a change notification carries: under full sync the last
/// content change is the whole document. `None` when either is missing.
fn changed_document(note: &Notification) -> Option<(i32, String)> {
    let version = note.params.pointer("/textDocument/version")?.as_i64()?;
    let changes = note.params.pointer("/contentChanges")?.as_array()?;
    let text = changes.last()?.get("text")?.as_str()?;
    Some((i32::try_from(version).ok()?, text.to_string()))
}

/// The file a document notification is about; `None` for any other notification or a URI
/// that is not a file.
fn document_path(note: &Notification) -> Option<PathBuf> {
    let uri = note.params.pointer("/textDocument/uri")?.as_str()?;
    path_of(&uri.parse().ok()?)
}
