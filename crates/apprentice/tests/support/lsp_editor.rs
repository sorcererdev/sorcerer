//! A small editor on the other end of an in-memory connection: it keeps its buffers, applies
//! the edits the server sends (checking versions, reporting the changes), answers progress and
//! documents as a real editor does, and the pickers from a queue.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use apprentice::adapters::lsp::{ServerConfig, serve};
use apprentice::ports::agent::Agent;
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::Diagnostic;
use serde_json::{Value, json};
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_lsp::uri_of;
use sorcerer_models::ports::decider::Decider;
use sorcerer_models::ports::researcher::Researcher;
use tempfile::TempDir;

use super::{FixedClock, open_with_vendored_grammars, test_cache};

/// How long a test waits for something the server must send.
pub const WAIT: Duration = Duration::from_secs(20);

/// The doubles a served apprentice runs with.
pub struct Doubles {
    pub agent: Arc<dyn Agent>,
    pub decider: Option<Arc<dyn Decider>>,
    pub researcher: Option<Arc<dyn Researcher>>,
}

pub struct Served {
    editor: Connection,
    pub root: PathBuf,
    server: Option<JoinHandle<()>>,
    inbox: RefCell<VecDeque<Message>>,
    requests_sent: std::cell::Cell<i32>,
    buffers: RefCell<HashMap<PathBuf, (i32, String)>>,
    /// What the pickers (`window/showMessageRequest`) answer, in order.
    pub picks: RefCell<VecDeque<String>>,
    pub picked_questions: RefCell<Vec<String>>,
    /// An editor with no picker: every `window/showMessageRequest` is answered with an error.
    pub no_picker: std::cell::Cell<bool>,
    /// An editor that ends every buffer it edits with a newline (Neovim's `eol`).
    pub adds_final_newline: std::cell::Cell<bool>,
    pub trust: TrustList,
    _trust_home: TempDir,
}

pub fn served(dir: &Path, doubles: Doubles) -> Served {
    let trust_home = TempDir::new().unwrap();
    let trust = TrustList::at(trust_home.path());
    let Doubles {
        agent,
        decider,
        researcher,
    } = doubles;
    let cfg = ServerConfig {
        open: Box::new(open_with_vendored_grammars),
        cache: test_cache(),
        trust: Some(trust.clone()),
        clock: Arc::new(FixedClock::thanksgiving()),
        agent: Arc::new(move |_| Ok(agent.clone())),
        decider: Box::new(move |_| decider.clone()),
        researcher: Box::new(move |_, _| researcher.clone()),
        idle: Duration::from_millis(50),
    };
    let (server_end, editor_end) = Connection::memory();
    let server = std::thread::spawn(move || serve(server_end, cfg).unwrap());
    let root = canonical(dir).unwrap();
    let s = Served {
        editor: editor_end,
        root: root.clone(),
        server: Some(server),
        inbox: RefCell::new(VecDeque::new()),
        requests_sent: std::cell::Cell::new(0),
        buffers: RefCell::new(HashMap::new()),
        picks: RefCell::new(VecDeque::new()),
        picked_questions: RefCell::new(Vec::new()),
        no_picker: std::cell::Cell::new(false),
        adds_final_newline: std::cell::Cell::new(false),
        trust,
        _trust_home: trust_home,
    };
    let capabilities = json!({
        "workspace": {
            "applyEdit": true,
            "workspaceEdit": { "documentChanges": true, "resourceOperations": ["create", "delete"] }
        },
        "window": { "workDoneProgress": true },
        "textDocument": { "publishDiagnostics": { "relatedInformation": true } }
    });
    let options = json!({
        "drawsWorking": true,
        "handles": ["sorcerer/ghost", "sorcerer/note", "sorcerer/showDiff"]
    });
    let id = s.request(
        "initialize",
        json!({ "capabilities": capabilities, "rootUri": uri_of(&root).as_str(), "initializationOptions": options }),
    );
    s.response(id);
    s.notify("initialized", json!({}));
    s
}

impl Served {
    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn uri(&self, relative: &str) -> String {
        uri_of(&self.path(relative)).as_str().to_string()
    }

    pub fn request(&self, method: &str, params: Value) -> i32 {
        let id = self.requests_sent.get() + 1;
        self.requests_sent.set(id);
        let request = Request::new(RequestId::from(id), method.into(), params);
        self.editor.sender.send(Message::Request(request)).unwrap();
        id
    }

    pub fn notify(&self, method: &str, params: Value) {
        let note = Notification::new(method.into(), params);
        self.editor
            .sender
            .send(Message::Notification(note))
            .unwrap();
    }

    fn reply(&self, id: RequestId, result: Value) {
        let response = Response {
            id,
            response_result: Ok(result),
        };
        self.editor
            .sender
            .send(Message::Response(response))
            .unwrap();
    }

    fn reply_error(&self, id: RequestId, method: &str) {
        let response = Response {
            id,
            response_result: Err(lsp_server::ResponseError {
                code: -32601,
                message: format!("{method} is not supported"),
                data: None,
            }),
        };
        self.editor
            .sender
            .send(Message::Response(response))
            .unwrap();
    }

    /// The editor opens a file from the disk at version 1.
    pub fn open(&self, relative: &str) {
        let path = self.path(relative);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        self.opened_with(&path, 1, &text);
    }

    fn opened_with(&self, path: &Path, version: i32, text: &str) {
        self.buffers
            .borrow_mut()
            .insert(path.to_path_buf(), (version, text.to_string()));
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": uri_of(path).as_str(), "languageId": "", "version": version, "text": text
            }}),
        );
    }

    /// The person types: the buffer's whole new text, one version on.
    pub fn type_text(&self, relative: &str, text: &str) {
        let path = self.path(relative);
        let version = self.buffers.borrow()[&path].0 + 1;
        self.buffers
            .borrow_mut()
            .insert(path.clone(), (version, text.to_string()));
        self.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri_of(&path).as_str(), "version": version },
                    "contentChanges": [{ "text": text }] }),
        );
    }

    pub fn text(&self, relative: &str) -> Option<String> {
        self.buffers
            .borrow()
            .get(&self.path(relative))
            .map(|(_, text)| text.clone())
    }

    /// The editor saves a buffer to the disk.
    pub fn save(&self, relative: &str) {
        let path = self.path(relative);
        std::fs::write(&path, self.text(relative).unwrap()).unwrap();
        self.notify(
            "textDocument/didSave",
            json!({ "textDocument": { "uri": uri_of(&path).as_str() } }),
        );
    }

    /// The next message; what a real editor answers by itself is answered here, and still
    /// handed to the test: a progress token, a document to show, an edit (applied), a picker
    /// (from `picks`).
    fn pull(&self, deadline: Instant) -> Option<Message> {
        let left = deadline.saturating_duration_since(Instant::now());
        let message = self.editor.receiver.recv_timeout(left).ok()?;
        if let Message::Request(r) = &message {
            match r.method.as_str() {
                "window/workDoneProgress/create" => self.reply(r.id.clone(), Value::Null),
                "window/showDocument" => self.reply(r.id.clone(), json!({ "success": true })),
                "workspace/applyEdit" => {
                    let applied = self.apply_quietly(&r.params["edit"]);
                    self.reply(r.id.clone(), json!({ "applied": applied.is_some() }));
                    for (method, params) in applied.unwrap_or_default() {
                        self.notify(&method, params);
                    }
                }
                "window/showMessageRequest" => {
                    self.picked_questions
                        .borrow_mut()
                        .push(r.params["message"].as_str().unwrap_or_default().to_string());
                    if self.no_picker.get() {
                        self.reply_error(r.id.clone(), "window/showMessageRequest");
                    } else {
                        let pick = self.picks.borrow_mut().pop_front();
                        let answer = pick.map_or(Value::Null, |title| json!({ "title": title }));
                        self.reply(r.id.clone(), answer);
                    }
                }
                _ => {}
            }
        }
        Some(message)
    }

    pub fn wait_for(&self, within: Duration, wanted: impl Fn(&Message) -> bool) -> Option<Message> {
        let waiting = self.inbox.borrow().iter().position(&wanted);
        if let Some(i) = waiting {
            return self.inbox.borrow_mut().remove(i);
        }
        let deadline = Instant::now() + within;
        while let Some(message) = self.pull(deadline) {
            if wanted(&message) {
                return Some(message);
            }
            self.inbox.borrow_mut().push_back(message);
        }
        None
    }

    pub fn response(&self, id: i32) -> Response {
        let id = RequestId::from(id);
        match self.wait_for(WAIT, |m| matches!(m, Message::Response(r) if r.id == id)) {
            Some(Message::Response(r)) => r,
            _ => panic!("no response to request {id}"),
        }
    }

    /// The next message shown that starts with `start`.
    pub fn shown_message(&self, start: &str) -> String {
        let message = self
            .wait_for(WAIT, |m| {
                matches!(m, Message::Notification(n) if n.method == "window/showMessage"
                    && n.params["message"].as_str().is_some_and(|text| text.starts_with(start)))
            })
            .unwrap_or_else(|| panic!("no message starting {start}"));
        let Message::Notification(note) = message else {
            unreachable!()
        };
        note.params["message"].as_str().unwrap().to_string()
    }

    /// How many messages starting with `start` were shown and not yet taken, after letting the
    /// server run a little.
    pub fn shown_count(&self, start: &str) -> usize {
        self.settle(Duration::from_millis(300));
        self.inbox
            .borrow()
            .iter()
            .filter(|m| {
                matches!(m, Message::Notification(n) if n.method == "window/showMessage"
                    && n.params["message"].as_str().is_some_and(|text| text.starts_with(start)))
            })
            .count()
    }

    /// The next notification of a method.
    pub fn notification(&self, method: &str) -> Value {
        let message = self
            .wait_for(
                WAIT,
                |m| matches!(m, Message::Notification(n) if n.method == method),
            )
            .unwrap_or_else(|| panic!("no {method}"));
        let Message::Notification(note) = message else {
            unreachable!()
        };
        note.params
    }

    /// The next diagnostics published for a file that `wanted` accepts.
    pub fn marks_until(
        &self,
        relative: &str,
        wanted: impl Fn(&[Diagnostic]) -> bool,
    ) -> Vec<Diagnostic> {
        let uri = self.uri(relative);
        let message = self
            .wait_for(WAIT, |m| match m {
                Message::Notification(n)
                    if n.method == "textDocument/publishDiagnostics" && n.params["uri"] == uri =>
                {
                    let marks: Vec<Diagnostic> =
                        serde_json::from_value(n.params["diagnostics"].clone()).unwrap();
                    wanted(&marks)
                }
                _ => false,
            })
            .unwrap_or_else(|| panic!("no such diagnostics for {relative}"));
        let Message::Notification(note) = message else {
            unreachable!()
        };
        serde_json::from_value(note.params["diagnostics"].clone()).unwrap()
    }

    /// The code actions on a line (from 1).
    pub fn actions(&self, relative: &str, line: u32) -> Vec<Value> {
        let id = self.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": self.uri(relative) },
                "range": { "start": { "line": line - 1, "character": 0 }, "end": { "line": line - 1, "character": 0 } },
                "context": { "diagnostics": [] }
            }),
        );
        self.response(id)
            .response_result
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    pub fn titles(&self, relative: &str, line: u32) -> Vec<String> {
        self.actions(relative, line)
            .iter()
            .map(|action| action["title"].as_str().unwrap().to_string())
            .collect()
    }

    /// Runs the action whose title starts with `title` on a line (from 1).
    pub fn run_action(&self, relative: &str, line: u32, title: &str) {
        let actions = self.actions(relative, line);
        let action = actions
            .iter()
            .find(|action| action["title"].as_str().unwrap().starts_with(title))
            .unwrap_or_else(|| panic!("no action {title} in {actions:?}"));
        let command = &action["command"];
        let id = self.request(
            "workspace/executeCommand",
            json!({ "command": command["command"], "arguments": command["arguments"] }),
        );
        self.response(id);
    }

    /// Lets the server run, keeping what it sends.
    pub fn settle(&self, within: Duration) {
        let deadline = Instant::now() + within;
        while let Some(message) = self.pull(deadline) {
            self.inbox.borrow_mut().push_back(message);
        }
    }

    /// Applies an edit to the buffers and the disk (creates, deletes) without telling the
    /// server: the notifications to send, or `None` when a version did not match.
    fn apply_quietly(&self, edit: &Value) -> Option<Vec<(String, Value)>> {
        let changes = edit["documentChanges"].as_array().unwrap();
        for change in changes {
            if let Some(version) = change["textDocument"]["version"].as_i64() {
                let path = path_of(&change["textDocument"]["uri"]);
                let have = self.buffers.borrow().get(&path).map(|(v, _)| i64::from(*v));
                if have != Some(version) {
                    return None;
                }
            }
        }
        let mut notes = Vec::new();
        for change in changes {
            match change["kind"].as_str() {
                Some("create") => {
                    let path = path_of(&change["uri"]);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, "").unwrap();
                    self.buffers
                        .borrow_mut()
                        .insert(path.clone(), (1, String::new()));
                    notes.push(opened_note(&path, 1, ""));
                }
                Some("delete") => {
                    let path = path_of(&change["uri"]);
                    let _ = std::fs::remove_file(&path);
                    self.buffers.borrow_mut().remove(&path);
                    notes.push((
                        "textDocument/didClose".to_string(),
                        json!({ "textDocument": { "uri": uri_of(&path).as_str() } }),
                    ));
                }
                _ => {
                    let path = path_of(&change["textDocument"]["uri"]);
                    if !self.buffers.borrow().contains_key(&path) {
                        let text = std::fs::read_to_string(&path).unwrap();
                        self.buffers
                            .borrow_mut()
                            .insert(path.clone(), (1, text.clone()));
                        notes.push(opened_note(&path, 1, &text));
                    }
                    let (version, text) = self.buffers.borrow()[&path].clone();
                    let new_text = self.with_final_newline(applied_edits(
                        &text,
                        change["edits"].as_array().unwrap(),
                    ));
                    self.buffers
                        .borrow_mut()
                        .insert(path.clone(), (version + 1, new_text.clone()));
                    notes.push((
                        "textDocument/didChange".to_string(),
                        json!({ "textDocument": { "uri": uri_of(&path).as_str(), "version": version + 1 },
                                "contentChanges": [{ "text": new_text }] }),
                    ));
                }
            }
        }
        Some(notes)
    }

    fn with_final_newline(&self, text: String) -> String {
        if self.adds_final_newline.get() && !text.is_empty() && !text.ends_with('\n') {
            text + "\n"
        } else {
            text
        }
    }

    pub fn stop(mut self) {
        let id = self.request("shutdown", Value::Null);
        self.response(id);
        self.notify("exit", Value::Null);
        self.server.take().unwrap().join().unwrap();
    }
}

fn opened_note(path: &Path, version: i32, text: &str) -> (String, Value) {
    (
        "textDocument/didOpen".to_string(),
        json!({ "textDocument": { "uri": uri_of(path).as_str(), "languageId": "", "version": version, "text": text } }),
    )
}

pub fn path_of(uri: &Value) -> PathBuf {
    sorcerer_lsp::path_of(&uri.as_str().unwrap().parse().unwrap()).unwrap()
}

/// A text with LSP text edits applied: positions are lines and UTF-16 units, the edits do not
/// overlap and are applied from the last.
fn applied_edits(text: &str, edits: &[Value]) -> String {
    let mut ranges: Vec<(usize, usize, String)> = edits
        .iter()
        .map(|e| {
            (
                offset_of(text, &e["range"]["start"]),
                offset_of(text, &e["range"]["end"]),
                e["newText"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    ranges.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut text = text.to_string();
    for (start, end, new) in ranges {
        text.replace_range(start..end, &new);
    }
    text
}

fn offset_of(text: &str, position: &Value) -> usize {
    let line = position["line"].as_u64().unwrap() as usize;
    let character = position["character"].as_u64().unwrap() as usize;
    let mut start = 0;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(at) => start += at + 1,
            None => return text.len(),
        }
    }
    let rest = &text[start..];
    let mut units = 0;
    for (at, c) in rest.char_indices() {
        if units >= character || c == '\n' {
            return start + at;
        }
        units += c.len_utf16();
    }
    text.len()
}
