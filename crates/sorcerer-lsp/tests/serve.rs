//! The host over an in-memory connection, with a product that records what reaches it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response, ResponseError};
use lsp_types::{MessageType, ServerCapabilities};
use serde_json::{Value, json};
use sorcerer_lsp::{Answer, Applied, Client, Product, Reply, serve, serve::unknown_method, uri_of};

const WAIT: Duration = Duration::from_secs(5);

/// The editor's workspace: an absolute path on every platform (it need not exist).
fn shop() -> PathBuf {
    std::env::temp_dir().join("work").join("shop")
}

/// The `file:` URI of a path, as text.
fn uri(path: &Path) -> String {
    uri_of(path).as_str().to_string()
}
const QUIET: Duration = Duration::from_millis(200);

/// What reached the product, in order, and the client it was given.
#[derive(Default)]
struct Seen {
    events: Vec<String>,
    client: Option<Client>,
    root: Option<PathBuf>,
}

struct Recording {
    seen: Arc<Mutex<Seen>>,
}

impl Recording {
    fn note(&self, event: String) {
        self.seen.lock().unwrap().events.push(event);
    }
}

impl Product for Recording {
    fn capabilities() -> ServerCapabilities {
        ServerCapabilities::default()
    }
    fn opened(&mut self, path: &Path) {
        let open = self.client().is_open(path);
        self.note(format!("opened {} (open: {open})", path.display()));
    }
    fn closed(&mut self, path: &Path) {
        let open = self.client().is_open(path);
        self.note(format!("closed {} (open: {open})", path.display()));
    }
    fn saved(&mut self, path: &Path) {
        self.note(format!("saved {}", path.display()));
    }
    /// Never called: this product does not track text.
    fn changed(&mut self, path: &Path, version: i32, _text: &str) {
        self.note(format!("changed {} v{version}", path.display()));
    }
    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        match method {
            "test/echo" => Ok(params),
            _ => Err(unknown_method(method)),
        }
    }
    fn shutdown(&mut self) {
        self.note("shutdown".into());
    }
}

impl Recording {
    fn client(&self) -> Client {
        self.seen.lock().unwrap().client.clone().unwrap()
    }
}

/// A product that tracks the open documents' text, and records each change with what the
/// client already says of it.
struct Tracking(Recording);

impl Product for Tracking {
    fn capabilities() -> ServerCapabilities {
        ServerCapabilities::default()
    }
    fn tracks_text() -> bool {
        true
    }
    fn opened(&mut self, path: &Path) {
        self.0.opened(path);
    }
    fn changed(&mut self, path: &Path, version: i32, text: &str) {
        let known = self.0.client().text_of(path);
        self.0
            .note(format!("changed v{version} {text} (text_of: {known:?})"));
    }
    fn closed(&mut self, path: &Path) {
        self.0.closed(path);
    }
    fn saved(&mut self, path: &Path) {
        self.0.saved(path);
    }
    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        self.0.request(method, params)
    }
    fn shutdown(&mut self) {
        self.0.shutdown();
    }
}

/// The editor's end of a server running on a thread.
struct Editor {
    connection: Connection,
    seen: Arc<Mutex<Seen>>,
    server: Option<JoinHandle<anyhow::Result<()>>>,
}

impl Editor {
    fn start(capabilities: Value) -> Editor {
        Editor::serving(capabilities, |seen| Recording { seen })
    }

    /// An editor that sent these `initializationOptions`.
    fn with_options(options: Value) -> Editor {
        Editor::serving_with(json!({}), options, |seen| Recording { seen })
    }

    /// An editor talking to the product `make` builds.
    fn serving<P: Product + 'static>(
        capabilities: Value,
        make: fn(Arc<Mutex<Seen>>) -> P,
    ) -> Editor {
        Editor::serving_with(capabilities, json!(null), make)
    }

    fn serving_with<P: Product + 'static>(
        capabilities: Value,
        options: Value,
        make: fn(Arc<Mutex<Seen>>) -> P,
    ) -> Editor {
        let (server_end, editor_end) = Connection::memory();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let for_server = Arc::clone(&seen);
        let server = std::thread::spawn(move || {
            serve(server_end, move |client, root| {
                let mut seen = for_server.lock().unwrap();
                seen.client = Some(client);
                seen.root = root;
                make(Arc::clone(&for_server))
            })
        });
        let editor = Editor {
            connection: editor_end,
            seen,
            server: Some(server),
        };
        editor.request(
            1,
            "initialize",
            json!({ "capabilities": capabilities, "rootUri": uri(&shop()),
                    "initializationOptions": options }),
        );
        editor.notify("initialized", json!({}));
        editor
    }

    fn plain() -> Editor {
        Editor::start(json!({}))
    }

    fn request(&self, id: i32, method: &str, params: Value) {
        let request = Request::new(RequestId::from(id), method.into(), params);
        self.connection
            .sender
            .send(Message::Request(request))
            .unwrap();
    }

    fn notify(&self, method: &str, params: Value) {
        let note = Notification::new(method.into(), params);
        self.connection
            .sender
            .send(Message::Notification(note))
            .unwrap();
    }

    fn document(&self, method: &str, uri: &str) {
        self.notify(method, json!({ "textDocument": { "uri": uri } }));
    }

    fn open_with_text(&self, uri: &str, version: i32, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "plaintext",
                                      "version": version, "text": text } }),
        );
    }

    /// A whole-document change, as an editor sends it under full sync.
    fn change(&self, uri: &str, version: i32, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": version },
                    "contentChanges": [{ "text": text }] }),
        );
    }

    fn reply(&self, id: RequestId, result: Result<Value, ResponseError>) {
        let response = Response {
            id,
            response_result: result,
        };
        self.connection
            .sender
            .send(Message::Response(response))
            .unwrap();
    }

    /// The next message from the server, or `None` within the wait.
    fn next(&self, within: Duration) -> Option<Message> {
        self.connection.receiver.recv_timeout(within).ok()
    }

    /// The response to request `id`, skipping anything else.
    fn response(&self, id: i32) -> Response {
        loop {
            match self.next(WAIT).expect("a response") {
                Message::Response(r) if r.id == RequestId::from(id) => return r,
                _ => {}
            }
        }
    }

    fn client(&self) -> Client {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            if let Some(client) = self.seen.lock().unwrap().client.clone() {
                return client;
            }
            assert!(std::time::Instant::now() < deadline, "no client");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn events(&self) -> Vec<String> {
        self.seen.lock().unwrap().events.clone()
    }

    /// Waits until the product has seen `count` events.
    fn wait_for_events(&self, count: usize) -> Vec<String> {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            let events = self.events();
            if events.len() >= count || std::time::Instant::now() > deadline {
                return events;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn stop(mut self) {
        self.request(99, "shutdown", Value::Null);
        self.response(99);
        self.notify("exit", Value::Null);
        self.server.take().unwrap().join().unwrap().unwrap();
    }
}

#[test]
fn the_handshake_announces_open_close_and_save_only() {
    let editor = Editor::plain();
    let Some(Message::Response(r)) = editor.next(WAIT) else {
        panic!("no initialize response")
    };
    let sync = &r.response_result.unwrap()["capabilities"]["textDocumentSync"];
    assert_eq!(sync["openClose"], json!(true));
    assert!(sync.get("change").is_none(), "{sync}");
    assert_eq!(sync["save"], json!({ "includeText": false }));
    editor.client();
    assert_eq!(editor.seen.lock().unwrap().root, Some(shop()));
    editor.stop();
}

#[test]
fn a_product_that_tracks_text_announces_full_sync() {
    let editor = Editor::serving(json!({}), |seen| Tracking(Recording { seen }));
    let Some(Message::Response(r)) = editor.next(WAIT) else {
        panic!("no initialize response")
    };
    let sync = &r.response_result.unwrap()["capabilities"]["textDocumentSync"];
    assert_eq!(sync["openClose"], json!(true));
    assert_eq!(sync["change"], json!(1));
    assert_eq!(sync["save"], json!({ "includeText": false }));
    editor.stop();
}

#[test]
fn open_and_change_keep_the_latest_version_and_text() {
    let editor = Editor::serving(json!({}), |seen| Tracking(Recording { seen }));
    let a = shop().join("a.txt");
    editor.open_with_text(&uri(&a), 1, "a");
    editor.change(&uri(&a), 2, "b");
    editor.change(&uri(&a), 3, "c");
    assert_eq!(
        editor.wait_for_events(3),
        vec![
            format!("opened {} (open: true)", a.display()),
            r#"changed v2 b (text_of: Some((2, "b")))"#.to_string(),
            r#"changed v3 c (text_of: Some((3, "c")))"#.to_string(),
        ]
    );
    assert_eq!(editor.client().text_of(&a), Some((3, "c".to_string())));
    editor.stop();
}

#[test]
fn close_forgets_the_text() {
    let editor = Editor::serving(json!({}), |seen| Tracking(Recording { seen }));
    let a = shop().join("a.txt");
    editor.open_with_text(&uri(&a), 1, "a");
    editor.document("textDocument/didClose", &uri(&a));
    editor.wait_for_events(2);
    assert_eq!(editor.client().text_of(&a), None);
    editor.stop();
}

#[test]
fn a_product_that_does_not_track_text_keeps_none() {
    let editor = Editor::plain();
    let a = shop().join("a.txt");
    editor.open_with_text(&uri(&a), 1, "a");
    editor.change(&uri(&a), 2, "b");
    editor.document("textDocument/didSave", &uri(&a));
    assert_eq!(
        editor.wait_for_events(2),
        vec![
            format!("opened {} (open: true)", a.display()),
            format!("saved {}", a.display()),
        ]
    );
    assert_eq!(editor.client().text_of(&a), None);
    editor.stop();
}

#[test]
fn the_first_workspace_folder_is_the_root() {
    let (server_end, editor_end) = Connection::memory();
    let root = Arc::new(Mutex::new(None));
    let for_server = Arc::clone(&root);
    let server = std::thread::spawn(move || {
        serve(server_end, move |_client, given| {
            *for_server.lock().unwrap() = Some(given);
            Recording {
                seen: Arc::default(),
            }
        })
    });
    let request = Request::new(
        RequestId::from(1),
        "initialize".into(),
        json!({ "capabilities": {}, "rootUri": uri(&shop().join("a")),
                "workspaceFolders": [{ "uri": uri(&shop().join("b")), "name": "b" }] }),
    );
    editor_end.sender.send(request.into()).unwrap();
    editor_end
        .sender
        .send(Notification::new("initialized".into(), json!({})).into())
        .unwrap();
    editor_end
        .sender
        .send(Notification::new("exit".into(), Value::Null).into())
        .unwrap();
    server.join().unwrap().unwrap();
    assert_eq!(*root.lock().unwrap(), Some(Some(shop().join("b"))));
}

#[test]
fn open_save_close_reach_the_product_and_the_open_set() {
    let editor = Editor::plain();
    let a = shop().join("a.txt");
    editor.document("textDocument/didOpen", &uri(&a));
    editor.document("textDocument/didSave", &uri(&a));
    editor.document("textDocument/didClose", &uri(&a));
    assert_eq!(
        editor.wait_for_events(3),
        vec![
            format!("opened {} (open: true)", a.display()),
            format!("saved {}", a.display()),
            format!("closed {} (open: false)", a.display()),
        ]
    );
    assert!(editor.client().open_paths().is_empty());
    editor.stop();
}

#[test]
fn a_save_of_a_non_file_uri_is_dropped() {
    let editor = Editor::plain();
    editor.document("textDocument/didOpen", "untitled:Untitled-1");
    editor.document(
        "textDocument/didSave",
        "fugitive:///work/shop/.git//0/a.txt",
    );
    let a = shop().join("a.txt");
    editor.document("textDocument/didSave", &uri(&a));
    assert_eq!(
        editor.wait_for_events(1),
        vec![format!("saved {}", a.display())]
    );
    editor.stop();
}

#[test]
fn a_request_is_answered_and_an_unknown_method_refused() {
    let editor = Editor::plain();
    editor.request(2, "test/echo", json!({ "x": 1 }));
    assert_eq!(
        editor.response(2).response_result.unwrap(),
        json!({ "x": 1 })
    );
    editor.request(3, "test/nothing", Value::Null);
    assert_eq!(editor.response(3).response_result.unwrap_err().code, -32601);
    editor.stop();
}

/// Every notification with this method the server sends within the quiet wait.
fn notifications(editor: &Editor, method: &str) -> Vec<Value> {
    std::iter::from_fn(|| editor.next(QUIET))
        .filter_map(|m| match m {
            Message::Notification(n) if n.method == method => Some(n.params),
            _ => None,
        })
        .collect()
}

#[test]
fn say_once_says_a_key_once() {
    let editor = Editor::plain();
    let client = editor.client();
    assert!(client.say_once("day", MessageType::WARNING, "spent"));
    assert!(!client.say_once("day", MessageType::WARNING, "spent again"));
    let shown = notifications(&editor, "window/showMessage");
    assert_eq!(shown, vec![json!({ "type": 2, "message": "spent" })]);
    editor.stop();
}

#[test]
fn say_says_every_time() {
    let editor = Editor::plain();
    let client = editor.client();
    client.say(MessageType::INFO, "busy");
    client.say(MessageType::INFO, "busy");
    let shown = notifications(&editor, "window/showMessage");
    assert_eq!(
        shown,
        vec![
            json!({ "type": 3, "message": "busy" }),
            json!({ "type": 3, "message": "busy" })
        ]
    );
    editor.stop();
}

#[test]
fn publish_and_log_reach_the_editor() {
    let editor = Editor::plain();
    let client = editor.client();
    let spaced = shop().join("a b.txt");
    client.publish(&spaced, Vec::new());
    let published = notifications(&editor, "textDocument/publishDiagnostics");
    assert_eq!(
        published,
        vec![json!({ "uri": uri(&spaced), "diagnostics": [] })]
    );
    client.log("hello");
    assert_eq!(
        notifications(&editor, "window/logMessage"),
        vec![json!({ "type": 4, "message": "hello" })]
    );
    editor.stop();
}

/// Asks a question and hands back the request the editor saw and where the answer lands.
fn asked(editor: &Editor) -> (Request, Arc<Mutex<Option<Answer>>>) {
    let answer = Arc::new(Mutex::new(None));
    let landed = Arc::clone(&answer);
    editor
        .client()
        .ask("Trust?", &["Trust", "Not now"], move |a| {
            *landed.lock().unwrap() = Some(a);
        });
    loop {
        match editor.next(WAIT).expect("a question") {
            Message::Request(r) if r.method == "window/showMessageRequest" => return (r, answer),
            _ => {}
        }
    }
}

fn answer_within(answer: &Arc<Mutex<Option<Answer>>>) -> Option<Answer> {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        if let Some(a) = answer.lock().unwrap().clone() {
            return Some(a);
        }
        if std::time::Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn ask_delivers_the_picked_answer() {
    let editor = Editor::plain();
    let (request, answer) = asked(&editor);
    assert_eq!(request.params["message"], json!("Trust?"));
    assert_eq!(
        request.params["actions"],
        json!([{ "title": "Trust" }, { "title": "Not now" }])
    );
    editor.reply(request.id, Ok(json!({ "title": "Trust" })));
    assert_eq!(answer_within(&answer), Some(Answer::Picked("Trust".into())));
    editor.stop();
}

#[test]
fn ask_delivers_closed_on_null() {
    let editor = Editor::plain();
    let (request, answer) = asked(&editor);
    editor.reply(request.id, Ok(Value::Null));
    assert_eq!(answer_within(&answer), Some(Answer::Closed));
    editor.stop();
}

#[test]
fn ask_delivers_unsupported_on_an_error_reply() {
    let editor = Editor::plain();
    let (request, answer) = asked(&editor);
    editor.reply(
        request.id,
        Err(ResponseError {
            code: -32601,
            message: "no".into(),
            data: None,
        }),
    );
    assert_eq!(answer_within(&answer), Some(Answer::Unsupported));
    editor.stop();
}

/// Applies an edit and hands back the request the editor saw and where its result lands.
fn applying(editor: &Editor) -> (Request, Arc<Mutex<Option<Applied>>>) {
    let applied = Arc::new(Mutex::new(None));
    let landed = Arc::clone(&applied);
    let a = shop().join("a.txt");
    let edit: lsp_types::WorkspaceEdit = serde_json::from_value(json!({
        "changes": { uri(&a): [{ "range": { "start": { "line": 0, "character": 0 },
                                            "end": { "line": 0, "character": 1 } },
                                 "newText": "b" }] }
    }))
    .unwrap();
    editor.client().apply_edit("conjure", edit, move |result| {
        *landed.lock().unwrap() = Some(result);
    });
    loop {
        match editor.next(WAIT).expect("an applyEdit request") {
            Message::Request(r) if r.method == "workspace/applyEdit" => return (r, applied),
            _ => {}
        }
    }
}

fn applied_within(applied: &Arc<Mutex<Option<Applied>>>) -> Option<Applied> {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        if let Some(a) = applied.lock().unwrap().clone() {
            return Some(a);
        }
        if std::time::Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn apply_edit_sends_the_edit_and_delivers_applied() {
    let editor = Editor::plain();
    let (request, applied) = applying(&editor);
    assert_eq!(request.params["label"], json!("conjure"));
    let a = uri(&shop().join("a.txt"));
    assert_eq!(
        request.params["edit"]["changes"][&a][0]["newText"],
        json!("b")
    );
    editor.reply(request.id, Ok(json!({ "applied": true })));
    assert_eq!(
        applied_within(&applied),
        Some(Applied {
            applied: true,
            failure_reason: None
        })
    );
    editor.stop();
}

#[test]
fn apply_edit_delivers_a_refusal_with_its_reason() {
    let editor = Editor::plain();
    let (request, applied) = applying(&editor);
    editor.reply(
        request.id,
        Ok(json!({ "applied": false, "failureReason": "version moved" })),
    );
    assert_eq!(
        applied_within(&applied),
        Some(Applied {
            applied: false,
            failure_reason: Some("version moved".into())
        })
    );
    editor.stop();
}

#[test]
fn apply_edit_delivers_not_applied_on_an_error_reply() {
    let editor = Editor::plain();
    let (request, applied) = applying(&editor);
    editor.reply(
        request.id,
        Err(ResponseError {
            code: -32601,
            message: "no applyEdit here".into(),
            data: None,
        }),
    );
    assert_eq!(
        applied_within(&applied),
        Some(Applied {
            applied: false,
            failure_reason: Some("no applyEdit here".into())
        })
    );
    editor.stop();
}

#[test]
fn an_editor_announcing_resource_operations_takes_document_changes() {
    // Neovim 0.12 applies documentChanges and announces resource operations, which exist only
    // inside documentChanges, but does not announce documentChanges itself
    let neovim = Editor::start(json!({ "workspace": {
        "applyEdit": true,
        "workspaceEdit": { "resourceOperations": ["rename", "create", "delete"] }
    } }));
    let client = neovim.client();
    assert!(client.can_apply_edits());
    assert!(client.can_create_files());
    assert!(client.can_delete_files());
    neovim.stop();

    let empty = Editor::start(json!({ "workspace": {
        "applyEdit": true,
        "workspaceEdit": { "resourceOperations": [] }
    } }));
    assert!(!empty.client().can_apply_edits());
    empty.stop();
}

#[test]
fn edit_abilities_are_read_at_initialize() {
    let none = Editor::plain();
    let client = none.client();
    assert!(!client.can_apply_edits());
    assert!(!client.can_create_files());
    assert!(!client.can_delete_files());
    none.stop();

    let apply_only = Editor::start(json!({ "workspace": { "applyEdit": true } }));
    assert!(
        !apply_only.client().can_apply_edits(),
        "documentChanges is needed too"
    );
    apply_only.stop();

    let changes_only =
        Editor::start(json!({ "workspace": { "workspaceEdit": { "documentChanges": true } } }));
    assert!(
        !changes_only.client().can_apply_edits(),
        "applyEdit is needed too"
    );
    changes_only.stop();

    let create_only = Editor::start(json!({ "workspace": {
        "applyEdit": true,
        "workspaceEdit": { "documentChanges": true, "resourceOperations": ["create"] }
    } }));
    let client = create_only.client();
    assert!(client.can_apply_edits());
    assert!(client.can_create_files());
    assert!(!client.can_delete_files());
    create_only.stop();

    let all = Editor::start(json!({ "workspace": {
        "applyEdit": true,
        "workspaceEdit": { "documentChanges": true,
                           "resourceOperations": ["create", "rename", "delete"] }
    } }));
    let client = all.client();
    assert!(client.can_apply_edits());
    assert!(client.can_create_files());
    assert!(client.can_delete_files());
    all.stop();
}

#[test]
fn work_done_is_silent_without_client_support() {
    let editor = Editor::plain();
    let line = editor.client().work_done("scry", "reviewing 1 unit");
    line.report("reviewing 2 units");
    line.end(None);
    let sent: Vec<Message> = std::iter::from_fn(|| editor.next(QUIET)).collect();
    assert!(
        sent.iter().all(|m| matches!(m, Message::Response(_))),
        "{sent:?}"
    );
    editor.stop();
}

#[test]
fn work_done_creates_begins_and_ends_with_it() {
    let editor = Editor::start(json!({ "window": { "workDoneProgress": true } }));
    let line = editor.client().work_done("scry", "reviewing 1 unit");
    let create = loop {
        if let Message::Request(r) = editor.next(WAIT).expect("a create request") {
            break r;
        }
    };
    assert_eq!(create.method, "window/workDoneProgress/create");
    assert!(
        notifications(&editor, "$/progress").is_empty(),
        "nothing before the editor made the token"
    );
    editor.reply(create.id, Ok(Value::Null));
    line.end(None);
    let progress = notifications(&editor, "$/progress");
    let token = &create.params["token"];
    assert_eq!(
        progress,
        vec![
            json!({ "token": token, "value": { "kind": "begin", "title": "scry", "message": "reviewing 1 unit" } }),
            json!({ "token": token, "value": { "kind": "end" } }),
        ]
    );
    editor.stop();
}

#[test]
fn shutdown_then_exit_ends_serve() {
    let editor = Editor::plain();
    editor.request(7, "shutdown", Value::Null);
    assert_eq!(editor.response(7).response_result.unwrap(), Value::Null);
    assert_eq!(editor.wait_for_events(1), vec!["shutdown"]);
    editor.stop();
}

#[test]
fn a_hang_up_with_a_question_unanswered_shuts_down_and_lets_the_connection_close() {
    let editor = Editor::plain();
    let client = editor.client();
    // The callback holds a client, as scry's trust question holds the server.
    let held = client.clone();
    client.ask("trust?", &["Trust"], move |_| drop(held));
    drop(client);
    assert!(matches!(editor.next(WAIT), Some(Message::Response(_))));
    assert!(matches!(editor.next(WAIT), Some(Message::Request(_))));
    let Editor {
        connection,
        seen,
        server,
    } = editor;
    let Connection { sender, receiver } = connection;
    drop(sender);
    server.unwrap().join().unwrap().unwrap();
    assert_eq!(seen.lock().unwrap().events, vec!["shutdown"]);
    // Every sender on the server's side is gone, so the editor's receiver sees the end.
    seen.lock().unwrap().client = None;
    assert!(receiver.recv_timeout(WAIT).unwrap_err().is_disconnected());
}

fn working_notes(editor: &Editor) -> Vec<Value> {
    notifications(editor, "sorcerer/working")
}

#[test]
fn working_is_silent_unless_the_editor_draws_it() {
    let editor = Editor::plain();
    let client = editor.client();
    client.show_working("scry: reviewing", &[(shop().join("a.py"), 3)]);
    client.clear_working();
    assert!(working_notes(&editor).is_empty());
    editor.stop();
}

#[test]
fn working_sends_each_place_with_its_line_from_zero_and_clears_with_an_empty_list() {
    let editor = Editor::with_options(json!({ "drawsWorking": true }));
    let client = editor.client();
    client.show_working(
        "scry: reviewing",
        &[(shop().join("a.py"), 3), (shop().join("b.py"), 1)],
    );
    client.clear_working();
    client.clear_working();
    let a = uri(&shop().join("a.py"));
    let b = uri(&shop().join("b.py"));
    assert_eq!(
        working_notes(&editor),
        vec![
            json!({ "units": [
                { "uri": a, "line": 2, "text": "scry: reviewing" },
                { "uri": b, "line": 0, "text": "scry: reviewing" },
            ] }),
            json!({ "units": [] }),
        ],
        "a clear with nothing shown says nothing"
    );
    editor.stop();
}

/// A product that answers `test/later` once the editor has applied an edit, and drops the
/// reply to `test/drop`.
struct Later(Recording);

impl Product for Later {
    fn capabilities() -> ServerCapabilities {
        ServerCapabilities::default()
    }
    fn opened(&mut self, path: &Path) {
        self.0.opened(path);
    }
    fn closed(&mut self, path: &Path) {
        self.0.closed(path);
    }
    fn saved(&mut self, path: &Path) {
        self.0.saved(path);
    }
    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        self.0.request(method, params)
    }
    fn request_later(&mut self, method: &str, params: Value, reply: Reply) {
        match method {
            "test/later" => self.0.client().apply_edit(
                "later",
                lsp_types::WorkspaceEdit::default(),
                move |applied| reply.send(Ok(json!({ "applied": applied.applied }))),
            ),
            "test/drop" => drop(reply),
            _ => reply.send(self.request(method, params)),
        }
    }
    fn shutdown(&mut self) {
        self.0.shutdown();
    }
}

#[test]
fn a_request_answered_later_waits_for_the_editors_reply() {
    let editor = Editor::serving(json!({}), |seen| Later(Recording { seen }));
    editor.request(2, "test/later", json!({}));
    let asked = loop {
        match editor.next(WAIT).expect("the edit") {
            Message::Request(r) if r.method == "workspace/applyEdit" => break r,
            Message::Response(r) if r.id == RequestId::from(2) => panic!("answered early"),
            _ => {}
        }
    };
    // the loop goes on while the answer waits
    editor.request(3, "test/echo", json!("still here"));
    assert_eq!(
        editor.response(3).response_result.unwrap(),
        json!("still here")
    );
    editor.reply(asked.id, Ok(json!({ "applied": true })));
    assert_eq!(
        editor.response(2).response_result.unwrap(),
        json!({ "applied": true })
    );
    editor.request(4, "test/echo", json!(1));
    assert_eq!(editor.response(4).response_result.unwrap(), json!(1));
    editor.stop();
}

#[test]
fn a_reply_dropped_without_an_answer_answers_with_an_error() {
    let editor = Editor::serving(json!({}), |seen| Later(Recording { seen }));
    editor.request(2, "test/drop", json!({}));
    let error = editor.response(2).response_result.expect_err("an error");
    assert!(
        error.message.contains("without an answer"),
        "{}",
        error.message
    );
    editor.stop();
}

#[test]
fn a_sorcerer_notification_reaches_only_an_editor_that_handles_it() {
    let plain = Editor::plain();
    assert!(
        !plain
            .client()
            .notify_sorcerer("sorcerer/ghost", json!({ "lines": [] }))
    );
    assert!(notifications(&plain, "sorcerer/ghost").is_empty());
    plain.stop();
    let editor = Editor::with_options(json!({ "handles": ["sorcerer/ghost", "other/thing"] }));
    let client = editor.client();
    assert!(client.handles("sorcerer/ghost"));
    assert!(!client.handles("other/thing"));
    assert!(client.notify_sorcerer("sorcerer/ghost", json!({ "lines": ["x"] })));
    assert!(!client.notify_sorcerer("sorcerer/note", json!({})));
    assert_eq!(
        notifications(&editor, "sorcerer/ghost"),
        [json!({ "lines": ["x"] })]
    );
    editor.stop();
}
