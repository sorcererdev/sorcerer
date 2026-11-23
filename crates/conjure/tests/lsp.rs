//! conjure's language server, driven over an in-memory connection by a small editor that
//! keeps its own buffers and applies the edits it is sent, with a scripted writer.
#![allow(clippy::too_many_lines)]
mod support;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Utc};
use conjure::adapters::lsp::{ServerConfig, serve};
use conjure::domain::record::{Attempt, ChangeRecord, Choice};
use conjure::ports::clock::Clock;
use conjure::ports::writer::{Writer, Written};
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};
use serde_json::{Value, json};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::language::GrammarSource;
use sorcerer_core::domain::project::Project;
use sorcerer_lsp::uri_of;
use support::{commit, errand, git_project, write};
use tempfile::TempDir;

/// How long a test waits for something the server must send.
const WAIT: Duration = Duration::from_secs(20);
/// How long a test watches for something the server must not send.
const QUIET: Duration = Duration::from_millis(300);

const A: &str = "import b\n\n\ndef total(cart):\n    pass\n\n\nX = 1\n";
const B: &str = "def helper():\n    return 1\n";

// ---- the writer and the clock ----

/// What the scripted writer answers, in order, whichever attempt asks.
#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<Result<String, String>>>,
    /// Each situation the writer was shown, with the attempt that showed it.
    seen: Mutex<Vec<(Attempt, String)>>,
    /// When set, a call waits for a message on it before it answers.
    gate: Mutex<Option<Receiver<()>>>,
}

impl Script {
    fn replying(replies: &[&str]) -> Arc<Script> {
        let script = Script::default();
        for reply in replies {
            script
                .replies
                .lock()
                .unwrap()
                .push_back(Ok((*reply).into()));
        }
        Arc::new(script)
    }

    fn seen(&self) -> Vec<(Attempt, String)> {
        self.seen.lock().unwrap().clone()
    }

    /// Waits until the writer has been asked (it has read the situation).
    fn wait_until_asked(&self) {
        let deadline = Instant::now() + WAIT;
        while self.seen.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Calls wait until the sender sends.
    fn gated(&self) -> Sender<()> {
        let (open, gate) = channel();
        *self.gate.lock().unwrap() = Some(gate);
        open
    }
}

struct ScriptedWriter {
    script: Arc<Script>,
    attempt: Attempt,
}

impl Writer for ScriptedWriter {
    fn write(&self, situation: &str, _: &Cancel) -> sorcerer_models::Result<Written> {
        self.script
            .seen
            .lock()
            .unwrap()
            .push((self.attempt, situation.into()));
        if let Some(gate) = self.script.gate.lock().unwrap().as_ref() {
            let _ = gate.recv_timeout(WAIT);
        }
        let reply = self.script.replies.lock().unwrap().pop_front();
        let verb = match self.attempt {
            Attempt::First => "conjure",
            Attempt::TryHarder => "try-harder",
        };
        match reply {
            Some(Ok(text)) => Ok(Written {
                text,
                errand: errand(verb, self.model(), 0.0012),
            }),
            Some(Err(why)) => Err(sorcerer_models::Error::Transport {
                provider: "stub".into(),
                why,
            }),
            None => Err(sorcerer_models::Error::Transport {
                provider: "stub".into(),
                why: "no reply scripted".into(),
            }),
        }
    }

    fn model(&self) -> &str {
        match self.attempt {
            Attempt::First => "haiku",
            Attempt::TryHarder => "sonnet",
        }
    }
}

/// A clock a second later each time it is read, so every change has its own id.
struct TickingClock {
    at: Mutex<DateTime<Utc>>,
}

impl Clock for TickingClock {
    fn now(&self) -> DateTime<Utc> {
        let mut at = self.at.lock().unwrap();
        *at += chrono::Duration::seconds(1);
        *at
    }

    fn offset(&self) -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }
}

// ---- the harness ----

/// What an editor that can do everything conjure uses says at initialize.
fn full_capabilities() -> Value {
    json!({
        "workspace": {
            "applyEdit": true,
            "workspaceEdit": { "documentChanges": true, "resourceOperations": ["create", "delete"] }
        },
        "window": { "workDoneProgress": true }
    })
}

struct Setup {
    capabilities: Value,
    /// The editor's `initializationOptions`.
    options: Value,
}

impl Default for Setup {
    fn default() -> Setup {
        Setup {
            capabilities: full_capabilities(),
            options: json!({}),
        }
    }
}

fn open_with_vendored_grammars(root: &Path) -> anyhow::Result<Project> {
    let mut project = Project::open(root)?;
    for name in ["python", "rust"] {
        project
            .languages
            .override_grammar(name, GrammarSource::Local(support::vendored(name)));
    }
    Ok(project)
}

/// The editor's end of a server running on a thread, with the editor's buffers.
struct Served {
    editor: Connection,
    root: PathBuf,
    script: Arc<Script>,
    server: Option<JoinHandle<()>>,
    inbox: RefCell<VecDeque<Message>>,
    requests_sent: std::cell::Cell<i32>,
    /// The documents the editor has open: version and text.
    buffers: RefCell<HashMap<PathBuf, (i32, String)>>,
    trust: TrustList,
    _trust_home: TempDir,
}

fn served(dir: &Path, script: Arc<Script>) -> Served {
    served_with(dir, script, Setup::default())
}

fn served_with(dir: &Path, script: Arc<Script>, setup: Setup) -> Served {
    let trust_home = TempDir::new().unwrap();
    let trust = TrustList::at(trust_home.path());
    let for_wire = Arc::clone(&script);
    let cfg = ServerConfig {
        open: Box::new(open_with_vendored_grammars),
        cache: support::test_cache(),
        trust: Some(trust.clone()),
        clock: Arc::new(TickingClock {
            at: Mutex::new("2026-11-22T12:00:00Z".parse().unwrap()),
        }),
        wire: Box::new(move |_project, _config, attempt, _view| {
            Ok(Box::new(ScriptedWriter {
                script: Arc::clone(&for_wire),
                attempt,
            }) as Box<dyn Writer>)
        }),
    };
    let (server_end, editor_end) = Connection::memory();
    let server = std::thread::spawn(move || serve(server_end, cfg).unwrap());
    let root = canonical(dir).unwrap();
    let s = Served {
        editor: editor_end,
        root: root.clone(),
        script,
        server: Some(server),
        inbox: RefCell::new(VecDeque::new()),
        requests_sent: std::cell::Cell::new(0),
        buffers: RefCell::new(HashMap::new()),
        trust,
        _trust_home: trust_home,
    };
    let id = s.request(
        "initialize",
        json!({ "capabilities": setup.capabilities, "rootUri": uri_of(&root).as_str(),
                "initializationOptions": setup.options }),
    );
    s.response(id);
    s.notify("initialized", json!({}));
    s
}

impl Served {
    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn uri(&self, relative: &str) -> String {
        uri_of(&self.path(relative)).as_str().to_string()
    }

    fn request(&self, method: &str, params: Value) -> i32 {
        let id = self.requests_sent.get() + 1;
        self.requests_sent.set(id);
        let request = Request::new(RequestId::from(id), method.into(), params);
        self.editor.sender.send(Message::Request(request)).unwrap();
        id
    }

    fn notify(&self, method: &str, params: Value) {
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

    /// The editor opens a file from the disk at version 1.
    fn open(&self, relative: &str) {
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
    fn type_text(&self, relative: &str, text: &str) {
        let path = self.path(relative);
        let version = self.buffers.borrow()[&path].0 + 1;
        self.changed_to(&path, version, text);
    }

    fn changed_to(&self, path: &Path, version: i32, text: &str) {
        self.buffers
            .borrow_mut()
            .insert(path.to_path_buf(), (version, text.to_string()));
        self.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri_of(path).as_str(), "version": version },
                    "contentChanges": [{ "text": text }] }),
        );
    }

    fn text(&self, relative: &str) -> Option<String> {
        self.buffers
            .borrow()
            .get(&self.path(relative))
            .map(|(_, text)| text.clone())
    }

    /// The editor saves a buffer to the disk.
    fn save(&self, relative: &str) {
        let path = self.path(relative);
        std::fs::write(&path, self.text(relative).unwrap()).unwrap();
        self.notify(
            "textDocument/didSave",
            json!({ "textDocument": { "uri": uri_of(&path).as_str() } }),
        );
    }

    fn close(&self, relative: &str) {
        let path = self.path(relative);
        self.buffers.borrow_mut().remove(&path);
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri_of(&path).as_str() } }),
        );
    }

    /// The next message from the server; a progress token and a document to show are answered
    /// as a real editor would, and still handed to the test.
    fn pull(&self, deadline: Instant) -> Option<Message> {
        let left = deadline.saturating_duration_since(Instant::now());
        let message = self.editor.receiver.recv_timeout(left).ok()?;
        if let Message::Request(r) = &message {
            match r.method.as_str() {
                "window/workDoneProgress/create" => self.reply(r.id.clone(), Value::Null),
                "window/showDocument" => self.reply(r.id.clone(), json!({ "success": true })),
                _ => {}
            }
        }
        Some(message)
    }

    fn wait_for(&self, within: Duration, wanted: impl Fn(&Message) -> bool) -> Option<Message> {
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

    fn all_within(&self, within: Duration) -> Vec<Message> {
        let deadline = Instant::now() + within;
        while let Some(message) = self.pull(deadline) {
            self.inbox.borrow_mut().push_back(message);
        }
        self.inbox.borrow_mut().drain(..).collect()
    }

    fn response(&self, id: i32) -> Response {
        let id = RequestId::from(id);
        match self.wait_for(WAIT, |m| matches!(m, Message::Response(r) if r.id == id)) {
            Some(Message::Response(r)) => r,
            _ => panic!("no response to request {id}"),
        }
    }

    /// The next message shown to the person.
    fn shown_message(&self) -> String {
        let message = self
            .wait_for(
                WAIT,
                |m| matches!(m, Message::Notification(n) if n.method == "window/showMessage"),
            )
            .expect("a message shown");
        let Message::Notification(note) = message else {
            unreachable!()
        };
        note.params["message"].as_str().unwrap().to_string()
    }

    /// The next diagnostics published for a file.
    fn marks(&self, relative: &str) -> Vec<Diagnostic> {
        let uri = self.uri(relative);
        let message = self
            .wait_for(WAIT, |m| is_publish_for(m, &uri))
            .unwrap_or_else(|| panic!("no diagnostics for {relative}"));
        let Message::Notification(note) = message else {
            unreachable!()
        };
        serde_json::from_value(note.params["diagnostics"].clone()).unwrap()
    }

    /// The titles of the code actions on a line (from 0) of a file, with the diagnostics the
    /// editor shows there.
    fn actions(
        &self,
        relative: &str,
        line: u32,
        end_line: u32,
        diagnostics: &[Diagnostic],
    ) -> Vec<Value> {
        let id = self.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": self.uri(relative) },
                "range": { "start": { "line": line, "character": 0 },
                           "end": { "line": end_line, "character": 0 } },
                "context": { "diagnostics": diagnostics }
            }),
        );
        let response = self.response(id);
        response
            .response_result
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    fn titles(&self, relative: &str, line: u32, diagnostics: &[Diagnostic]) -> Vec<String> {
        titles_of(&self.actions(relative, line, line, diagnostics))
    }

    /// Runs the command a code action carries.
    fn run(&self, action: &Value) -> Response {
        let command = &action["command"];
        let id = self.request(
            "workspace/executeCommand",
            json!({ "command": command["command"], "arguments": command["arguments"] }),
        );
        self.response(id)
    }

    /// Asks for *Conjure here* on a line (from 1) and runs it.
    fn conjure_here(&self, relative: &str, line: u32) -> Response {
        let actions = self.actions(relative, line - 1, line - 1, &[]);
        let here = actions
            .iter()
            .find(|a| a["title"] == "Conjure here")
            .unwrap_or_else(|| panic!("no Conjure here in {actions:?}"));
        self.run(here)
    }

    /// The next `workspace/applyEdit` the server sends: its id and edit.
    fn edit_sent(&self) -> (RequestId, Value) {
        let message = self
            .wait_for(
                WAIT,
                |m| matches!(m, Message::Request(r) if r.method == "workspace/applyEdit"),
            )
            .expect("an edit sent");
        let Message::Request(request) = message else {
            unreachable!()
        };
        (request.id, request.params)
    }

    /// The editor applies the next edit as a real one would: versions checked, each change
    /// made in its buffer (a file not open is opened from the disk first), then the open and
    /// change notifications, then the answer. Returns the edit.
    fn apply_next_edit(&self) -> Value {
        let (id, params) = self.edit_sent();
        let applied = self.apply(&params["edit"]);
        self.reply(id, json!({ "applied": applied }));
        params
    }

    /// The editor applies the next edit and keeps every file with CRLF lines, as VS Code on
    /// Windows does for a new file.
    fn apply_next_edit_with_crlf_lines(&self) -> Value {
        let (id, params) = self.edit_sent();
        let applied = self.apply_quietly(&params["edit"]);
        let to_crlf = |text: &str| text.replace("\r\n", "\n").replace('\n', "\r\n");
        let mut notes = applied.clone().unwrap_or_default();
        for (_, note) in &mut notes {
            let uri = note["textDocument"]["uri"].clone();
            let Some(text) = note["contentChanges"][0]["text"]
                .as_str()
                .or_else(|| note["textDocument"]["text"].as_str())
                .map(to_crlf)
            else {
                continue;
            };
            let path = path_of(&uri);
            if let Some(buffer) = self.buffers.borrow_mut().get_mut(&path) {
                buffer.1 = text.clone();
            }
            if note["contentChanges"][0]["text"].is_string() {
                note["contentChanges"][0]["text"] = json!(text);
            } else {
                note["textDocument"]["text"] = json!(text);
            }
        }
        self.reply(id, json!({ "applied": applied.is_some() }));
        self.report(Some(notes));
        params
    }

    /// The editor refuses the next edit.
    fn refuse_next_edit(&self) -> Value {
        let (id, params) = self.edit_sent();
        self.reply(
            id,
            json!({ "applied": false, "failureReason": "version moved" }),
        );
        params
    }

    fn apply(&self, edit: &Value) -> bool {
        let applied = self.apply_quietly(edit);
        self.report(applied.clone());
        applied.is_some()
    }

    /// The editor answers first and reports its changes a moment later, as Neovim does (it
    /// debounces didChange).
    fn apply_next_edit_answering_first(&self) -> Value {
        let (id, params) = self.edit_sent();
        let applied = self.apply_quietly(&params["edit"]);
        self.reply(id, json!({ "applied": applied.is_some() }));
        std::thread::sleep(Duration::from_millis(150));
        self.report(applied);
        params
    }

    /// The notifications of an applied edit, sent.
    fn report(&self, applied: Option<Vec<(String, Value)>>) {
        for (method, params) in applied.unwrap_or_default() {
            self.notify(&method, params);
        }
    }

    /// Applies an edit to the buffers and the disk (creates, deletes) without telling the
    /// server: the notifications to send, or `None` when a version did not match.
    fn apply_quietly(&self, edit: &Value) -> Option<Vec<(String, Value)>> {
        let changes = edit["documentChanges"].as_array().unwrap();
        // every version checked before anything changes, as an editor applies it whole
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
        let opened = |path: &Path, version: i32, text: &str| {
            self.buffers
                .borrow_mut()
                .insert(path.to_path_buf(), (version, text.to_string()));
            (
                "textDocument/didOpen".to_string(),
                json!({ "textDocument": {
                    "uri": uri_of(path).as_str(), "languageId": "", "version": version, "text": text
                }}),
            )
        };
        for change in changes {
            match change["kind"].as_str() {
                Some("create") => {
                    let path = path_of(&change["uri"]);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, "").unwrap();
                    notes.push(opened(&path, 1, ""));
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
                        notes.push(opened(&path, 1, &text));
                    }
                    let (version, text) = self.buffers.borrow()[&path].clone();
                    let edits = change["edits"].as_array().unwrap();
                    let new_text = applied_edits(&text, edits);
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

    /// The records written so far.
    fn records(&self) -> Vec<ChangeRecord> {
        let file = self.root.join(".sorcerer/state/conjure/changes.jsonl");
        std::fs::read_to_string(file)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// Lets the server run for a moment, keeping what it sends.
    fn settle(&self) {
        let deadline = Instant::now() + QUIET;
        while let Some(message) = self.pull(deadline) {
            self.inbox.borrow_mut().push_back(message);
        }
    }

    /// Waits until the records hold N lines.
    fn records_reach(&self, n: usize) -> Vec<ChangeRecord> {
        let deadline = Instant::now() + WAIT;
        loop {
            let records = self.records();
            if records.len() >= n || Instant::now() > deadline {
                return records;
            }
            // the server writes from the main loop; let it run, keeping what it sends
            let deadline = Instant::now() + Duration::from_millis(20);
            while let Some(message) = self.pull(deadline) {
                self.inbox.borrow_mut().push_back(message);
            }
        }
    }

    fn stop(mut self) {
        let id = self.request("shutdown", Value::Null);
        self.response(id);
        self.notify("exit", Value::Null);
        self.server.take().unwrap().join().unwrap();
    }
}

fn path_of(uri: &Value) -> PathBuf {
    sorcerer_lsp::path_of(&uri.as_str().unwrap().parse().unwrap()).unwrap()
}

fn is_publish_for(m: &Message, uri: &str) -> bool {
    matches!(m, Message::Notification(n)
        if n.method == "textDocument/publishDiagnostics" && n.params["uri"] == uri)
}

fn titles_of(actions: &[Value]) -> Vec<String> {
    actions
        .iter()
        .map(|a| a["title"].as_str().unwrap().to_string())
        .collect()
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

// ---- the project ----

/// A git project: `src/a.py` with a stubbed `total`, `src/b.py`, a README.
fn project() -> TempDir {
    let dir = git_project();
    write(dir.path(), "src/a.py", A);
    write(dir.path(), "src/b.py", B);
    write(dir.path(), "README.md", "# shop\n");
    commit(dir.path(), "shop");
    dir
}

// ---- Task 19: the actions offered, the trust gate ----

#[test]
fn the_server_tracks_text() {
    let dir = project();
    let (server_end, editor_end) = Connection::memory();
    let trust_home = TempDir::new().unwrap();
    let cfg = ServerConfig {
        open: Box::new(open_with_vendored_grammars),
        cache: support::test_cache(),
        trust: Some(TrustList::at(trust_home.path())),
        clock: Arc::new(TickingClock {
            at: Mutex::new("2026-11-22T12:00:00Z".parse().unwrap()),
        }),
        wire: Box::new(|_, _, _, _| anyhow::bail!("no writer")),
    };
    let server = std::thread::spawn(move || serve(server_end, cfg).unwrap());
    let init = Request::new(
        RequestId::from(1),
        "initialize".into(),
        json!({ "capabilities": full_capabilities(), "rootUri": uri_of(dir.path()).as_str() }),
    );
    editor_end.sender.send(Message::Request(init)).unwrap();
    let Message::Response(response) = editor_end.receiver.recv_timeout(WAIT).unwrap() else {
        panic!()
    };
    let capabilities = &response.response_result.unwrap()["capabilities"];
    assert_eq!(
        capabilities["textDocumentSync"]["change"], 1,
        "{capabilities}"
    );
    assert_eq!(capabilities["codeActionProvider"], true);
    let commands = &capabilities["executeCommandProvider"]["commands"];
    for command in [
        "conjure.conjure",
        "conjure.undo",
        "conjure.tryHarder",
        "conjure.why",
        "conjure.show",
    ] {
        assert!(
            commands.as_array().unwrap().contains(&json!(command)),
            "{commands}"
        );
    }
    for (method, params) in [("initialized", json!({})), ("exit", Value::Null)] {
        let note = Notification::new(method.into(), params);
        editor_end.sender.send(Message::Notification(note)).unwrap();
    }
    server.join().unwrap();
}

#[test]
fn conjure_here_is_offered_on_a_line_in_a_unit() {
    let dir = project();
    let s = served(dir.path(), Script::replying(&[]));
    s.open("src/a.py");
    // line 5 (from 1) is the body of `total`
    let actions = s.actions("src/a.py", 4, 4, &[]);
    assert_eq!(titles_of(&actions), ["Conjure here"]);
    let command = &actions[0]["command"];
    assert_eq!(command["command"], "conjure.conjure");
    assert_eq!(command["arguments"], json!([s.uri("src/a.py"), 5]));
    // a selection from the start of line 4 to the start of line 6 holds lines 4 and 5
    let selected = s.actions("src/a.py", 3, 5, &[]);
    assert_eq!(
        selected[0]["command"]["arguments"],
        json!([s.uri("src/a.py"), 4, 5])
    );
    // line 8 is in no unit
    assert!(s.titles("src/a.py", 7, &[]).is_empty());
    s.stop();
}

#[test]
fn nothing_is_offered_outside_the_projects_languages() {
    let dir = project();
    let s = served(dir.path(), Script::replying(&[]));
    s.open("README.md");
    assert!(s.titles("README.md", 0, &[]).is_empty());
    s.stop();
}

/// scry's findings file with one finding of STATUS on line 5 of `src/a.py`.
fn seed_finding(dir: &Path, id: &str, status: &str) {
    write(
        dir,
        ".sorcerer/state/scry/findings.json",
        &format!(
            r#"{{"version": 2, "data": {{"findings": [{{"id": "{id}", "issue_type": "bug", "rule": null,
            "statement": "total ignores the cart", "place": {{"file": "src/a.py", "unit": "total",
            "lines": {{"start": 5, "end": 5}}, "fingerprint": null}}, "evidence": [], "status": "{status}"}}]}}}}"#
        ),
    );
}

#[test]
fn conjure_for_this_finding_is_offered_on_an_open_findings_line() {
    let dir = project();
    seed_finding(dir.path(), "3fa9c0", "open");
    let s = served(dir.path(), Script::replying(&[]));
    s.open("src/a.py");
    let actions = s.actions("src/a.py", 4, 4, &[]);
    assert_eq!(
        titles_of(&actions),
        ["Conjure here", "Conjure for this finding"]
    );
    assert_eq!(
        actions[1]["command"]["arguments"],
        json!([s.uri("src/a.py"), 5, null, "3fa9c0"])
    );
    // the line above holds no finding
    assert_eq!(s.titles("src/a.py", 3, &[]), ["Conjure here"]);
    s.stop();
    // a dismissed finding offers nothing
    let dir = project();
    seed_finding(dir.path(), "3fa9c0", "dismissed");
    let s = served(dir.path(), Script::replying(&[]));
    s.open("src/a.py");
    assert_eq!(s.titles("src/a.py", 4, &[]), ["Conjure here"]);
    s.stop();
}

#[test]
fn an_editor_without_document_changes_is_offered_nothing_and_told_once() {
    let dir = project();
    let setup = Setup {
        capabilities: json!({ "workspace": { "applyEdit": true } }),
        ..Setup::default()
    };
    let s = served_with(dir.path(), Script::replying(&[]), setup);
    s.open("src/a.py");
    assert_eq!(
        s.shown_message(),
        "conjure: this editor cannot apply edits from a server (workspace/applyEdit with documentChanges)"
    );
    s.open("src/b.py");
    assert!(s.titles("src/a.py", 4, &[]).is_empty());
    let messages = s.all_within(QUIET);
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::Notification(n) if n.method == "window/showMessage")),
        "told twice: {messages:?}"
    );
    s.stop();
}

const RUNNER: &str = "[model.providers.mine]\nrunner = \"/x/run\"\n";

#[test]
fn an_untrusted_project_is_refused_with_conjure_trust() {
    let dir = project();
    write(dir.path(), ".sorcerer/config.toml", RUNNER);
    let s = served(dir.path(), Script::replying(&["<goal>g</goal>"]));
    s.open("src/a.py");
    // offered, then refused when run
    let response = s.conjure_here("src/a.py", 5);
    assert!(response.response_result.is_ok(), "{response:?}");
    assert_eq!(
        s.shown_message(),
        "conjure: this project is not trusted; run \"conjure trust\" in it"
    );
    assert!(s.script.seen().is_empty());
    s.stop();
}

// ---- Task 20: one edit over every file, the marks ----

const NEW_A: &str = "import b\n\n\ndef total(cart):\n    return sum(cart)\n\n\nX = 1\n";
const NEW_B: &str = "def helper():\n    return 2\n";
const NEW_TEST: &str =
    "from a import total\n\n\ndef test_total():\n    assert total([1, 2]) == 3\n";

/// A reply that fills `total` in the open `a.py`, changes the unopened `b.py` and creates
/// `a__test.py`.
fn three_files() -> String {
    format!(
        "<goal>fill total</goal>\n<file path=\"src/a.py\">\n{NEW_A}</file>\n<file path=\"src/b.py\">\n{NEW_B}</file>\n<file path=\"src/a__test.py\">\n{NEW_TEST}</file>\n"
    )
}

/// A served project with `a.py` open, asked for a change in `total` that the editor applies.
fn landed(replies: &[&str]) -> (TempDir, Served) {
    let dir = project();
    let s = served(dir.path(), Script::replying(replies));
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.apply_next_edit();
    (dir, s)
}

fn kinds_and_uris(edit: &Value) -> Vec<(String, String, Value)> {
    edit["documentChanges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| match change["kind"].as_str() {
            Some(kind) => (
                kind.to_string(),
                change["uri"].as_str().unwrap().into(),
                Value::Null,
            ),
            None => (
                "edit".into(),
                change["textDocument"]["uri"].as_str().unwrap().into(),
                change["textDocument"]["version"].clone(),
            ),
        })
        .collect()
}

#[test]
fn one_edit_carries_every_file() {
    let dir = project();
    // the writer writes whole files, so it echoes the typing it read
    let reply = three_files().replace("X = 1", "X = 2");
    let s = served(dir.path(), Script::replying(&[&reply]));
    s.open("src/a.py");
    // unsaved typing: the writer reads it, and the edit is made at the version it was read at
    let typed = A.replace("X = 1", "X = 2");
    s.type_text("src/a.py", &typed);
    let response = s.conjure_here("src/a.py", 5);
    assert!(response.response_result.is_ok(), "{response:?}");
    let params = s.apply_next_edit();
    assert_eq!(params["label"], "conjure: fill total");
    assert_eq!(
        kinds_and_uris(&params["edit"]),
        [
            ("edit".to_string(), s.uri("src/a.py"), json!(2)),
            ("edit".to_string(), s.uri("src/b.py"), Value::Null),
            ("create".to_string(), s.uri("src/a__test.py"), Value::Null),
            ("edit".to_string(), s.uri("src/a__test.py"), Value::Null),
        ]
    );
    assert!(s.script.seen()[0].1.contains("X = 2"));
    // only the line the writer changed is edited
    let a_edits = &params["edit"]["documentChanges"][0]["edits"];
    assert_eq!(a_edits.as_array().unwrap().len(), 1, "{a_edits}");
    assert_eq!(a_edits[0]["range"]["start"]["line"], 4);
    assert_eq!(a_edits[0]["range"]["end"]["line"], 5);
    assert_eq!(s.text("src/a.py").unwrap(), NEW_A.replace("X = 1", "X = 2"));
    assert_eq!(s.text("src/b.py").unwrap(), NEW_B);
    assert_eq!(s.text("src/a__test.py").unwrap(), NEW_TEST);
    // nothing written to disk
    assert_eq!(std::fs::read_to_string(s.path("src/a.py")).unwrap(), A);
    assert_eq!(std::fs::read_to_string(s.path("src/b.py")).unwrap(), B);
    s.stop();
}

#[test]
fn a_dropped_path_never_reaches_the_editor() {
    let reply = format!(
        "{}<file path=\"../x.py\">\nx = 1\n</file>\n<file path=\".env\">\nKEY=1\n</file>\n",
        three_files()
    );
    let (dir, s) = landed(&[&reply]);
    let marks = s.marks("src/a.py");
    assert!(
        marks[0].message.ends_with(" · not written: ../x.py, .env"),
        "{}",
        marks[0].message
    );
    let parent = dir.path().parent().unwrap().join("x.py");
    assert!(!parent.exists());
    assert!(!s.path(".env").exists());
    assert!(s.text(".env").is_none());
    s.stop();
}

#[test]
fn the_edit_names_no_dropped_path() {
    let dir = project();
    let reply = format!("{}<file path=\".env\">\nKEY=1\n</file>\n", three_files());
    let s = served(dir.path(), Script::replying(&[&reply]));
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    let (_, params) = s.edit_sent();
    let uris: Vec<String> = kinds_and_uris(&params["edit"])
        .into_iter()
        .map(|(_, uri, _)| uri)
        .collect();
    assert!(!uris.iter().any(|uri| uri.ends_with(".env")), "{uris:?}");
    s.stop();
}

#[test]
fn the_marks_name_the_goal_and_the_other_files() {
    let (_dir, s) = landed(&[&three_files()]);
    let main = s.marks("src/a.py");
    assert_eq!(main.len(), 1, "{main:?}");
    let mark = &main[0];
    assert_eq!(
        mark.message,
        "fill total · haiku · $0.001 · 0 s · parses · also changed: b.py, a__test.py"
    );
    assert_eq!(mark.range.start.line, 4);
    assert_eq!(mark.range.start.character, 0);
    assert_eq!(mark.range.end.line, 4);
    assert_eq!(
        mark.range.end.character,
        "    return sum(cart)".len() as u32
    );
    assert_eq!(mark.source.as_deref(), Some("conjure"));
    assert_eq!(mark.severity, Some(DiagnosticSeverity::INFORMATION));
    assert_eq!(mark.code, Some(NumberOrString::String("change".into())));
    let id = mark.data.clone().unwrap();
    let other = s.marks("src/b.py");
    assert_eq!(other[0].message, "conjured with a.py: fill total");
    assert_eq!(other[0].range.start.line, 1);
    assert_eq!(other[0].data, Some(id.clone()));
    let test = s.marks("src/a__test.py");
    assert_eq!(test[0].message, "conjured with a.py: fill total");
    assert_eq!(test[0].range.start.line, 0);
    s.stop();
}

#[test]
fn a_refused_edit_says_your_text_stands_and_marks_nothing() {
    let dir = project();
    let s = served(dir.path(), Script::replying(&[&three_files()]));
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.refuse_next_edit();
    assert_eq!(
        s.shown_message(),
        "conjure: the file changed while the change was conjured; your text stands"
    );
    let records = s.records_reach(1);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].choice, Choice::Dropped);
    let messages = s.all_within(QUIET);
    let marked = messages.iter().any(|m| {
        matches!(m, Message::Notification(n)
        if n.method == "textDocument/publishDiagnostics"
            && !n.params["diagnostics"].as_array().unwrap().is_empty())
    });
    assert!(!marked, "{messages:?}");
    s.stop();
}

/// The editor draws the units being worked on.
fn drawing_working() -> Setup {
    Setup {
        options: json!({ "drawsWorking": true }),
        ..Setup::default()
    }
}

impl Served {
    /// The units of the next `sorcerer/working`.
    fn working(&self) -> Option<Vec<Value>> {
        let message = self.wait_for(
            WAIT,
            |m| matches!(m, Message::Notification(n) if n.method == "sorcerer/working"),
        )?;
        let Message::Notification(note) = message else {
            unreachable!()
        };
        Some(note.params["units"].as_array().unwrap().clone())
    }
}

#[test]
fn the_unit_being_written_is_shown_inline_until_the_edit_is_answered() {
    let dir = project();
    let s = served_with(
        dir.path(),
        Script::replying(&[&three_files()]),
        drawing_working(),
    );
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    // `total` starts on line 4, so line 3 from 0
    assert_eq!(
        s.working(),
        Some(vec![json!({ "uri": s.uri("src/a.py"), "line": 3,
                          "text": "\u{27f3} conjure: writing\u{2026}" })])
    );
    s.apply_next_edit();
    assert_eq!(s.working(), Some(vec![]));
    s.stop();
}

#[test]
fn a_refused_edit_clears_the_inline_text() {
    let dir = project();
    let s = served_with(
        dir.path(),
        Script::replying(&[&three_files()]),
        drawing_working(),
    );
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    assert_eq!(s.working().map(|u| u.len()), Some(1));
    s.refuse_next_edit();
    assert_eq!(s.working(), Some(vec![]));
    s.stop();
}

#[test]
fn nothing_is_drawn_inline_for_an_editor_that_did_not_ask() {
    let dir = project();
    let s = served(dir.path(), Script::replying(&[&three_files()]));
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.apply_next_edit();
    let said = s.all_within(QUIET);
    assert!(
        !said
            .iter()
            .any(|m| matches!(m, Message::Notification(n) if n.method == "sorcerer/working")),
        "{said:?}"
    );
    s.stop();
}

/// Whether the server sent an edit within a moment.
fn sent_an_edit(s: &Served) -> bool {
    s.all_within(QUIET)
        .iter()
        .any(|m| matches!(m, Message::Request(r) if r.method == "workspace/applyEdit"))
}

#[test]
fn typing_while_conjuring_keeps_the_edit_from_being_sent() {
    let dir = project();
    let script = Script::replying(&[&three_files()]);
    let open = script.gated();
    let s = served(dir.path(), script);
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    // the person types while the writer writes: the change was made from version 1
    s.type_text("src/a.py", &A.replace("X = 1", "X = 3"));
    open.send(()).unwrap();
    assert_eq!(
        s.shown_message(),
        "conjure: the file changed while the change was conjured; your text stands"
    );
    assert!(!sent_an_edit(&s));
    assert_eq!(s.text("src/a.py").unwrap(), A.replace("X = 1", "X = 3"));
    let records = s.records_reach(1);
    assert_eq!(records[0].choice, Choice::Dropped);
    s.stop();
}

#[test]
fn a_file_opened_and_typed_in_while_conjuring_is_not_overwritten() {
    let dir = project();
    let script = Script::replying(&[&three_files()]);
    let open = script.gated();
    let s = served(dir.path(), script);
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    // b.py was not open when the change was asked for; now it is, with the person's typing
    s.open("src/b.py");
    s.type_text("src/b.py", "def helper():\n    return 9\n");
    open.send(()).unwrap();
    assert_eq!(
        s.shown_message(),
        "conjure: the file changed while the change was conjured; your text stands"
    );
    assert!(!sent_an_edit(&s));
    assert_eq!(s.text("src/b.py").unwrap(), "def helper():\n    return 9\n");
    s.stop();
}

#[test]
fn a_file_opened_unchanged_while_conjuring_still_lands() {
    let dir = project();
    let script = Script::replying(&[&three_files()]);
    let open = script.gated();
    let s = served(dir.path(), script);
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.open("src/b.py");
    open.send(()).unwrap();
    s.apply_next_edit();
    assert_eq!(s.text("src/b.py").unwrap(), NEW_B);
    s.stop();
}

#[test]
fn a_second_ask_while_one_runs_is_refused() {
    let dir = project();
    let script = Script::replying(&[&three_files(), &three_files()]);
    let open = script.gated();
    let s = served(dir.path(), script);
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    let second = s.conjure_here("src/a.py", 5);
    assert!(second.response_result.is_ok(), "{second:?}");
    assert_eq!(
        s.shown_message(),
        "conjure: a change is already being conjured; this one was not started"
    );
    open.send(()).unwrap();
    s.apply_next_edit();
    s.marks("src/a.py");
    assert_eq!(s.script.seen().len(), 1);
    s.stop();
}

#[test]
fn progress_shows_while_conjuring() {
    let (_dir, s) = landed(&[&three_files()]);
    s.marks("src/a.py");
    let messages = s.all_within(QUIET);
    let begun = messages.iter().any(|m| {
        matches!(m, Message::Notification(n)
        if n.method == "$/progress" && n.params["value"]["kind"] == "begin"
            && n.params["value"]["title"] == "conjure"
            && n.params["value"]["message"] == "conjuring a change")
    });
    let ended = messages.iter().any(|m| {
        matches!(m, Message::Notification(n)
        if n.method == "$/progress" && n.params["value"]["kind"] == "end")
    });
    assert!(begun && ended, "{messages:?}");
    s.stop();
}

#[test]
fn nothing_to_do_marks_no_change_on_the_units_first_line() {
    let dir = project();
    let s = served(
        dir.path(),
        Script::replying(&["<goal>total needs nothing</goal>\n"]),
    );
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    let marks = s.marks("src/a.py");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].message, "no change · total needs nothing");
    assert_eq!(marks[0].range.start.line, 3);
    assert_eq!(
        marks[0].code,
        Some(NumberOrString::String("no-change".into()))
    );
    assert_eq!(
        s.titles("src/a.py", 3, &marks),
        ["conjure: Try harder", "conjure: Show why", "Conjure here"]
    );
    let records = s.records_reach(1);
    assert_eq!(records[0].choice, Choice::NoChange);
    assert_eq!(records[0].goal.as_deref(), Some("total needs nothing"));
    // the next edit clears it
    s.type_text("src/a.py", &A.replace("X = 1", "X = 2"));
    assert!(s.marks("src/a.py").is_empty());
    s.stop();
}

#[test]
fn the_next_change_records_the_previous_kept() {
    let second = format!(
        "<goal>count the cart</goal>\n<file path=\"src/a.py\">\n{}</file>\n",
        NEW_A.replace("X = 1", "X = 0")
    );
    let (_dir, s) = landed(&[&three_files(), &second]);
    assert_eq!(s.marks("src/b.py").len(), 1);
    assert_eq!(s.marks("src/a__test.py").len(), 1);
    s.conjure_here("src/a.py", 5);
    s.apply_next_edit();
    let records = s.records_reach(1);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].choice, Choice::Kept);
    assert_eq!(records[0].goal.as_deref(), Some("fill total"));
    assert_eq!(records[0].files.len(), 3);
    assert_eq!(records[0].model, "haiku");
    // the first change's other marks are gone; the second's is on a.py
    assert!(s.marks("src/b.py").is_empty());
    assert!(s.marks("src/a__test.py").is_empty());
    s.stop();
}

#[test]
fn an_editor_that_cannot_create_files_gets_the_rest() {
    let dir = project();
    let setup = Setup {
        capabilities: json!({
            "workspace": { "applyEdit": true, "workspaceEdit": { "documentChanges": true } }
        }),
        ..Setup::default()
    };
    let s = served_with(dir.path(), Script::replying(&[&three_files()]), setup);
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    let params = s.apply_next_edit();
    let kinds: Vec<String> = kinds_and_uris(&params["edit"])
        .into_iter()
        .map(|(kind, uri, _)| format!("{kind} {uri}"))
        .collect();
    assert_eq!(
        kinds,
        [
            format!("edit {}", s.uri("src/a.py")),
            format!("edit {}", s.uri("src/b.py"))
        ]
    );
    assert_eq!(
        s.shown_message(),
        "conjure: this editor cannot create files; not created: src/a__test.py"
    );
    let main = s.marks("src/a.py");
    assert!(
        main[0].message.ends_with("parses · also changed: b.py"),
        "{}",
        main[0].message
    );
    s.stop();
}

#[test]
fn a_spent_day_says_its_sentence() {
    let dir = project();
    let spent = sorcerer_core::domain::errand::Errand {
        at: "2026-11-22T09:00:00Z".parse().unwrap(),
        ..errand("conjure", "haiku", 3.0)
    };
    write(
        dir.path(),
        ".sorcerer/state/ledger.jsonl",
        &format!("{}\n", serde_json::to_string(&spent).unwrap()),
    );
    let s = served(dir.path(), Script::replying(&[&three_files()]));
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    assert_eq!(
        s.shown_message(),
        "conjure: today's budget ($3.00) is spent. Conjuring resumes tomorrow; \"conjure cost\" shows the days, and [conjure] budget_per_day sets the budget."
    );
    assert!(s.script.seen().is_empty());
    s.stop();
}

#[test]
fn outside_a_unit_says_its_sentence() {
    let dir = project();
    let s = served(dir.path(), Script::replying(&[&three_files()]));
    s.open("src/a.py");
    let id = s.request(
        "workspace/executeCommand",
        json!({ "command": "conjure.conjure", "arguments": [s.uri("src/a.py"), 8] }),
    );
    s.response(id);
    assert_eq!(s.shown_message(), "put the cursor in a function to conjure");
    s.stop();
}

#[test]
fn a_writer_that_does_not_answer_is_said_and_recorded_failed() {
    let dir = project();
    let script = Script::default();
    script
        .replies
        .lock()
        .unwrap()
        .push_back(Err("timed out after 120 s".into()));
    let s = served(dir.path(), Arc::new(script));
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    assert_eq!(
        s.shown_message(),
        "conjure: the model did not answer: stub: timed out after 120 s"
    );
    let records = s.records_reach(1);
    assert_eq!(records[0].choice, Choice::Failed);
    assert_eq!(records[0].unit, "total");
    s.stop();
}

#[test]
fn a_trusted_project_conjures() {
    let dir = project();
    write(dir.path(), ".sorcerer/config.toml", RUNNER);
    let s = served(dir.path(), Script::replying(&[&three_files()]));
    s.trust.trust(&s.root).unwrap();
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.apply_next_edit();
    assert_eq!(s.marks("src/a.py").len(), 1);
    s.stop();
}

#[test]
fn line_endings_and_an_unterminated_last_line_survive_the_edit() {
    let dir = project();
    write(
        dir.path(),
        "src/c.py",
        "def f():\r\n    pass\r\n\r\n\r\nY = 1\r\n",
    );
    write(dir.path(), "src/d.py", "def g():\n    pass\n\n\nZ = 'é'");
    commit(dir.path(), "c and d");
    // the writer answers with LF lines and a final newline, as models do
    let reply = "<goal>fill f and g</goal>\n<file path=\"src/c.py\">\ndef f():\n    return 1\n\n\nY = 1\n</file>\n<file path=\"src/d.py\">\ndef g():\n    return 2\n\n\nZ = 'é'\n</file>\n";
    let s = served(dir.path(), Script::replying(&[reply]));
    s.open("src/c.py");
    s.conjure_here("src/c.py", 2);
    s.apply_next_edit();
    assert_eq!(
        s.text("src/c.py").unwrap(),
        "def f():\r\n    return 1\r\n\r\n\r\nY = 1\r\n"
    );
    assert_eq!(
        s.text("src/d.py").unwrap(),
        "def g():\n    return 2\n\n\nZ = 'é'"
    );
    s.stop();
}

#[test]
fn a_changed_unterminated_last_line_ends_in_utf16_units() {
    let dir = project();
    write(dir.path(), "src/d.py", "def g():\n    pass\n\n\nZ = 'é'");
    commit(dir.path(), "d");
    let reply = "<goal>name z</goal>\n<file path=\"src/d.py\">\ndef g():\n    pass\n\n\nZ = 'éé'\n</file>\n";
    let s = served(dir.path(), Script::replying(&[reply]));
    s.open("src/d.py");
    s.conjure_here("src/d.py", 2);
    let params = s.apply_next_edit();
    let edit = &params["edit"]["documentChanges"][0]["edits"][0];
    assert_eq!(edit["range"]["end"], json!({ "line": 4, "character": 7 }));
    assert_eq!(
        s.text("src/d.py").unwrap(),
        "def g():\n    pass\n\n\nZ = 'éé'\n"
    );
    s.stop();
}

// ---- Task 21: the mark's actions and the choices ----

/// The action titled TITLE on the main mark's line of `a.py`, with the marks the editor shows.
fn mark_action(s: &Served, relative: &str, marks: &[Diagnostic], title: &str) -> Value {
    let line = marks[0].range.start.line;
    let actions = s.actions(relative, line, line, marks);
    actions
        .into_iter()
        .find(|a| a["title"] == title)
        .unwrap_or_else(|| panic!("no {title}"))
}

#[test]
fn the_actions_come_in_order() {
    let (_dir, s) = landed(&[&three_files()]);
    let main = s.marks("src/a.py");
    assert_eq!(
        s.titles("src/a.py", 4, &main),
        [
            "conjure: Undo",
            "conjure: Try harder",
            "conjure: Show why",
            "conjure: Show b.py",
            "conjure: Show a__test.py",
            "Conjure here"
        ]
    );
    let other = s.marks("src/b.py");
    assert_eq!(
        s.titles("src/b.py", 1, &other),
        [
            "conjure: Undo",
            "conjure: Try harder",
            "conjure: Show why",
            "conjure: Show a.py",
            "conjure: Show a__test.py",
            "Conjure here"
        ]
    );
    // the editor moved the mark with the text: its id still finds the change
    let mut moved = main.clone();
    moved[0].range.start.line = 0;
    moved[0].range.end.line = 0;
    assert_eq!(s.titles("src/a.py", 0, &moved)[0], "conjure: Undo");
    s.stop();
}

#[test]
fn undo_reverts_every_file_in_one_edit_and_records_undone() {
    let (_dir, s) = landed(&[&three_files()]);
    let main = s.marks("src/a.py");
    assert_eq!(s.marks("src/b.py").len(), 1);
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Undo"));
    let params = s.apply_next_edit();
    assert_eq!(
        kinds_and_uris(&params["edit"]),
        [
            ("edit".to_string(), s.uri("src/a.py"), json!(2)),
            ("edit".to_string(), s.uri("src/b.py"), json!(2)),
            ("delete".to_string(), s.uri("src/a__test.py"), Value::Null),
        ]
    );
    assert_eq!(s.text("src/a.py").unwrap(), A);
    assert_eq!(s.text("src/b.py").unwrap(), B);
    let records = s.records_reach(1);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].choice, Choice::Undone);
    assert!(s.marks("src/a.py").is_empty());
    assert!(s.marks("src/b.py").is_empty());
    s.stop();
}

#[test]
fn a_change_the_editor_keeps_with_crlf_lines_landed_and_undoes_without_changing_the_endings() {
    let dir = project();
    let s = served(dir.path(), Script::replying(&[&three_files()]));
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.apply_next_edit_with_crlf_lines();
    let crlf = |text: &str| text.replace('\n', "\r\n");
    assert_eq!(s.text("src/a.py").unwrap(), crlf(NEW_A));
    let main = s.marks("src/a.py");
    assert!(
        main.iter()
            .all(|mark| !mark.message.contains("did not land")),
        "{main:?}"
    );
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Undo"));
    s.apply_next_edit();
    assert_eq!(s.text("src/a.py").unwrap(), crlf(A));
    assert_eq!(s.text("src/b.py").unwrap(), crlf(B));
    assert!(s.text("src/a__test.py").is_none());
    let records = s.records_reach(1);
    assert_eq!(records[0].choice, Choice::Undone);
    s.stop();
}

#[test]
fn undo_deletes_a_created_file() {
    let (_dir, s) = landed(&[&three_files()]);
    assert!(s.path("src/a__test.py").exists());
    let main = s.marks("src/a.py");
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Undo"));
    s.apply_next_edit();
    assert!(!s.path("src/a__test.py").exists());
    assert!(s.text("src/a__test.py").is_none());
    s.stop();
}

#[test]
fn undo_empties_a_created_file_when_the_editor_cannot_delete() {
    let dir = project();
    let setup = Setup {
        capabilities: json!({ "workspace": { "applyEdit": true,
            "workspaceEdit": { "documentChanges": true, "resourceOperations": ["create"] } } }),
        ..Setup::default()
    };
    let s = served_with(dir.path(), Script::replying(&[&three_files()]), setup);
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.apply_next_edit();
    let main = s.marks("src/a.py");
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Undo"));
    let params = s.apply_next_edit();
    let kinds: Vec<String> = kinds_and_uris(&params["edit"])
        .into_iter()
        .map(|(kind, _, _)| kind)
        .collect();
    assert_eq!(kinds, ["edit", "edit", "edit"]);
    assert_eq!(s.text("src/a__test.py").unwrap(), "");
    assert_eq!(
        s.shown_message(),
        "conjure: this editor cannot delete files; emptied: src/a__test.py"
    );
    s.stop();
}

#[test]
fn undo_after_typing_in_a_changed_file_is_refused_and_undoes_nothing() {
    let (_dir, s) = landed(&[&three_files()]);
    let main = s.marks("src/a.py");
    let typed = NEW_B.replace("return 2", "return 3");
    s.type_text("src/b.py", &typed);
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Undo"));
    assert_eq!(
        s.shown_message(),
        "conjure: b.py was edited after the change landed; undo it with the editor's own undo"
    );
    let messages = s.all_within(QUIET);
    let sent = messages
        .iter()
        .any(|m| matches!(m, Message::Request(r) if r.method == "workspace/applyEdit"));
    assert!(!sent, "{messages:?}");
    assert_eq!(s.text("src/b.py").unwrap(), typed);
    assert_eq!(s.text("src/a.py").unwrap(), NEW_A);
    assert!(s.records().is_empty());
    s.stop();
}

#[test]
fn try_harder_undoes_then_lands_the_stronger_change_with_the_same_situation() {
    let harder = format!(
        "<goal>fill total properly</goal>\n<file path=\"src/a.py\">\n{}</file>\n",
        NEW_A.replace("sum(cart)", "sum(item.price for item in cart)")
    );
    let (_dir, s) = landed(&[&three_files(), &harder]);
    let main = s.marks("src/a.py");
    let first_id = main[0].data.clone().unwrap();
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Try harder"));
    // the undo first, then the stronger change
    s.apply_next_edit();
    s.apply_next_edit();
    assert_eq!(
        s.text("src/a.py").unwrap(),
        NEW_A.replace("sum(cart)", "sum(item.price for item in cart)")
    );
    assert_eq!(s.text("src/b.py").unwrap(), B);
    let seen = s.script.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].0, Attempt::TryHarder);
    assert_eq!(seen[1].1, seen[0].1);
    let records = s.records_reach(1);
    assert_eq!(records[0].choice, Choice::Replaced);
    let marks = s.marks("src/a.py");
    let marks = if marks.is_empty() {
        s.marks("src/a.py")
    } else {
        marks
    };
    assert!(
        marks[0]
            .message
            .starts_with("fill total properly · sonnet · "),
        "{}",
        marks[0].message
    );
    s.save("src/a.py");
    let records = s.records_reach(2);
    assert_eq!(records[1].choice, Choice::Kept);
    assert_eq!(records[1].attempt, Attempt::TryHarder);
    assert_eq!(records[1].try_harder_of.as_deref(), first_id.as_str());
    s.stop();
}

#[test]
fn try_harder_from_no_change_asks_again_without_undoing() {
    let dir = project();
    let s = served(
        dir.path(),
        Script::replying(&["<goal>nothing to do</goal>\n", &three_files()]),
    );
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    let marks = s.marks("src/a.py");
    s.run(&mark_action(&s, "src/a.py", &marks, "conjure: Try harder"));
    s.apply_next_edit();
    assert_eq!(s.text("src/a.py").unwrap(), NEW_A);
    assert_eq!(s.script.seen()[1].0, Attempt::TryHarder);
    s.stop();
}

#[test]
fn try_harder_is_not_offered_when_switched_off() {
    let dir = project();
    write(
        dir.path(),
        ".sorcerer/config.toml",
        "[conjure]\ntry_harder = \"\"\n",
    );
    let s = served(dir.path(), Script::replying(&[&three_files()]));
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.apply_next_edit();
    let main = s.marks("src/a.py");
    assert_eq!(
        s.titles("src/a.py", 4, &main),
        [
            "conjure: Undo",
            "conjure: Show why",
            "conjure: Show b.py",
            "conjure: Show a__test.py",
            "Conjure here"
        ]
    );
    s.stop();
}

/// The next `window/showDocument` request's parameters.
fn shown_document(s: &Served) -> Value {
    let message = s
        .wait_for(
            WAIT,
            |m| matches!(m, Message::Request(r) if r.method == "window/showDocument"),
        )
        .expect("a document shown");
    let Message::Request(request) = message else {
        unreachable!()
    };
    request.params
}

#[test]
fn show_why_writes_the_file_and_opens_it() {
    let (_dir, s) = landed(&[&three_files()]);
    let main = s.marks("src/a.py");
    let id = main[0].data.clone().unwrap();
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Show why"));
    let why = s.path(&format!(
        ".sorcerer/state/conjure/why-{}.md",
        id.as_str().unwrap()
    ));
    assert_eq!(shown_document(&s)["uri"], uri_of(&why).as_str());
    let text = std::fs::read_to_string(&why).unwrap();
    for part in [
        "# fill total",
        "haiku · $0.001 · 0 s",
        "src/a.py",
        "src/a__test.py (new)",
        "## The situation",
        "## The reply",
        "## The facts",
        "<goal>fill total</goal>",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    assert!(text.contains(&s.script.seen()[0].1), "{text}");
    s.stop();
}

#[test]
fn show_file_opens_the_change_selected() {
    let (_dir, s) = landed(&[&three_files()]);
    let main = s.marks("src/a.py");
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Show b.py"));
    let shown = shown_document(&s);
    assert_eq!(shown["uri"], s.uri("src/b.py"));
    assert_eq!(
        shown["selection"],
        json!({ "start": { "line": 1, "character": 0 }, "end": { "line": 2, "character": 0 } })
    );
    s.stop();
}

#[test]
fn a_save_keeps_the_change_and_the_other_files_marks_stay() {
    let (_dir, s) = landed(&[&three_files()]);
    assert_eq!(s.marks("src/a.py").len(), 1);
    assert_eq!(s.marks("src/b.py").len(), 1);
    assert_eq!(s.marks("src/a__test.py").len(), 1);
    s.save("src/b.py");
    let records = s.records_reach(1);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].choice, Choice::Kept);
    // the saved file's mark goes; the unsaved files keep theirs, saying the change is kept
    assert!(s.marks("src/b.py").is_empty());
    let main = s.marks("src/a.py");
    assert_eq!(main.len(), 1);
    assert!(
        main[0]
            .message
            .ends_with(" · kept: a file was saved, so no Undo"),
        "{}",
        main[0].message
    );
    assert_eq!(
        s.titles("src/a.py", 4, &main),
        [
            "conjure: Show why",
            "conjure: Show b.py",
            "conjure: Show a__test.py",
            "Conjure here"
        ]
    );
    // each goes when its file is saved or closed, and nothing more is recorded
    s.save("src/a.py");
    s.close("src/a__test.py");
    s.settle();
    let messages: Vec<Message> = s.inbox.borrow_mut().drain(..).collect();
    let last_for = |relative: &str| {
        let uri = s.uri(relative);
        messages
            .iter()
            .rev()
            .find(|m| is_publish_for(m, &uri))
            .map(|m| match m {
                Message::Notification(n) => n.params["diagnostics"].as_array().unwrap().len(),
                _ => unreachable!(),
            })
    };
    assert_eq!(last_for("src/a.py"), Some(0));
    assert_eq!(last_for("src/a__test.py"), Some(0));
    assert_eq!(s.records().len(), 1);
    s.stop();
}

#[test]
fn the_editors_own_undo_records_undone() {
    let (_dir, s) = landed(&[&three_files()]);
    assert_eq!(s.marks("src/a.py").len(), 1);
    s.type_text("src/a.py", A);
    s.type_text("src/b.py", B);
    // one file back is not the change undone
    s.settle();
    assert!(s.records().is_empty());
    s.type_text("src/a__test.py", "");
    let records = s.records_reach(1);
    assert_eq!(records[0].choice, Choice::Undone);
    assert!(s.marks("src/a.py").is_empty());
    s.stop();
}

#[test]
fn shutdown_records_unanswered() {
    let (dir, s) = landed(&[&three_files()]);
    s.marks("src/a.py");
    s.stop();
    let text =
        std::fs::read_to_string(dir.path().join(".sorcerer/state/conjure/changes.jsonl")).unwrap();
    let record: ChangeRecord = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(record.choice, Choice::Unanswered);
}

#[test]
fn marks_follow_the_open_set() {
    let (_dir, s) = landed(&[&three_files()]);
    assert_eq!(s.marks("src/b.py").len(), 1);
    s.close("src/b.py");
    assert!(s.marks("src/b.py").is_empty());
    s.open("src/b.py");
    assert_eq!(s.marks("src/b.py").len(), 1);
    // closing records nothing
    assert!(s.records().is_empty());
    s.stop();
}

#[test]
fn try_harder_waits_for_the_editor_to_report_the_undo() {
    let harder = format!(
        "<goal>fill total properly</goal>\n<file path=\"src/a.py\">\n{}</file>\n",
        NEW_A.replace("sum(cart)", "sum(item.price for item in cart)")
    );
    let (_dir, s) = landed(&[&three_files(), &harder]);
    let main = s.marks("src/a.py");
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Try harder"));
    s.apply_next_edit_answering_first();
    s.apply_next_edit();
    assert_eq!(
        s.text("src/a.py").unwrap(),
        NEW_A.replace("sum(cart)", "sum(item.price for item in cart)")
    );
    let records = s.records_reach(1);
    assert_eq!(records[0].choice, Choice::Replaced);
    s.stop();
}

// ---- review fixes ----

#[test]
fn an_untrusted_projects_own_grammar_is_never_loaded_for_actions() {
    let dir = project();
    write(
        dir.path(),
        ".sorcerer/languages/toy/language.toml",
        "files = [\"**/*.toy\"]\ngrammar = { path = \"g\" }\n",
    );
    write(dir.path(), "src/a.toy", "toy\n");
    let s = served(dir.path(), Script::replying(&[]));
    s.open("src/a.toy");
    // untrusted: offered without parsing, so the trust sentence can be said when it is run
    assert_eq!(s.titles("src/a.toy", 0, &[]), ["Conjure here"]);
    let messages = s.all_within(QUIET);
    let logged_grammar = messages.iter().any(|m| {
        matches!(m, Message::Notification(n)
        if n.method == "window/logMessage")
    });
    assert!(!logged_grammar, "{messages:?}");
    assert!(!support::test_cache().dir().unwrap().join("toy").exists());
    // trusted, the file is parsed: the grammar (missing here) is tried, and no unit is found
    s.trust.trust(&s.root).unwrap();
    assert!(s.titles("src/a.toy", 0, &[]).is_empty());
    s.stop();
}

#[test]
fn a_file_rewritten_on_disk_while_conjuring_keeps_the_edit_from_being_sent() {
    let dir = project();
    let script = Script::replying(&[&three_files()]);
    let open = script.gated();
    let s = served(dir.path(), script);
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.script.wait_until_asked();
    // a formatter, or a branch switch, rewrites the unopened b.py meanwhile
    write(dir.path(), "src/b.py", "def helper():\n\n    return 1\n");
    open.send(()).unwrap();
    assert_eq!(
        s.shown_message(),
        "conjure: the file changed while the change was conjured; your text stands"
    );
    assert!(!sent_an_edit(&s));
    s.stop();
}

#[test]
fn a_new_file_that_appeared_while_conjuring_keeps_the_edit_from_being_sent() {
    let dir = project();
    let script = Script::replying(&[&three_files()]);
    let open = script.gated();
    let s = served(dir.path(), script);
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    s.script.wait_until_asked();
    write(dir.path(), "src/a__test.py", "def test_mine():\n    pass\n");
    open.send(()).unwrap();
    assert_eq!(
        s.shown_message(),
        "conjure: the file changed while the change was conjured; your text stands"
    );
    assert!(!sent_an_edit(&s));
    assert_eq!(
        std::fs::read_to_string(s.path("src/a__test.py")).unwrap(),
        "def test_mine():\n    pass\n"
    );
    s.stop();
}

#[test]
fn an_edit_the_editor_applied_in_part_is_said_and_offers_no_undo() {
    let dir = project();
    let s = served(dir.path(), Script::replying(&[&three_files()]));
    s.open("src/a.py");
    s.conjure_here("src/a.py", 5);
    // as Neovim does with a stale first file: skips it, applies the rest, answers applied
    let (id, params) = s.edit_sent();
    let mut edit = params["edit"].clone();
    edit["documentChanges"].as_array_mut().unwrap().remove(0);
    let notes = s.apply_quietly(&edit);
    s.report(notes);
    s.reply(id, json!({ "applied": true }));
    assert_eq!(
        s.shown_message(),
        "conjure: the change did not land as sent in a.py; the other files were changed, and conjure will not undo them"
    );
    let main = s.marks("src/a.py");
    assert!(
        main[0]
            .message
            .ends_with(" · did not land as sent: a.py, so no Undo"),
        "{}",
        main[0].message
    );
    assert_eq!(
        s.titles("src/a.py", 4, &main),
        [
            "conjure: Show why",
            "conjure: Show b.py",
            "conjure: Show a__test.py",
            "Conjure here"
        ]
    );
    let records = s.records_reach(1);
    assert_eq!(records[0].choice, Choice::LandedInPart);
    s.stop();
}

#[test]
fn conjure_here_while_an_undo_is_in_flight_is_busy_and_every_change_is_recorded() {
    let harder = format!(
        "<goal>fill total properly</goal>\n<file path=\"src/a.py\">\n{}</file>\n",
        NEW_A.replace("sum(cart)", "sum(item.price for item in cart)")
    );
    let (_dir, s) = landed(&[&three_files(), &harder]);
    let main = s.marks("src/a.py");
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Try harder"));
    let (undo_id, undo) = s.edit_sent();
    // before the editor answers the undo, the person asks again
    s.conjure_here("src/a.py", 5);
    assert_eq!(
        s.shown_message(),
        "conjure: a change is already being conjured; this one was not started"
    );
    let notes = s.apply_quietly(&undo["edit"]);
    s.report(notes);
    s.reply(undo_id, json!({ "applied": true }));
    s.apply_next_edit();
    let records = s.records_reach(1);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].choice, Choice::Replaced);
    assert_eq!(s.script.seen().len(), 2);
    s.stop();
}

#[test]
fn try_harder_of_a_one_file_change_is_recorded_replaced() {
    // the editor reports the undo's change before it answers (VS Code's order), and the one
    // file is then back at its text before: that is the undo, not the person's
    let one = format!("<goal>fill total</goal>\n<file path=\"src/a.py\">\n{NEW_A}</file>\n");
    let harder = format!(
        "<goal>fill total properly</goal>\n<file path=\"src/a.py\">\n{}</file>\n",
        NEW_A.replace("sum(cart)", "sum(item.price for item in cart)")
    );
    let (_dir, s) = landed(&[&one, &harder]);
    let main = s.marks("src/a.py");
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Try harder"));
    s.apply_next_edit();
    s.apply_next_edit();
    let records = s.records_reach(1);
    assert_eq!(records[0].choice, Choice::Replaced);
    assert_eq!(
        s.text("src/a.py").unwrap(),
        NEW_A.replace("sum(cart)", "sum(item.price for item in cart)")
    );
    s.stop();
}

#[test]
fn try_harder_refuses_when_the_editor_never_reports_the_undo() {
    let (_dir, s) = landed(&[&three_files(), &three_files()]);
    let main = s.marks("src/a.py");
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Try harder"));
    let (id, undo) = s.edit_sent();
    let _never_reported = s.apply_quietly(&undo["edit"]);
    s.reply(id, json!({ "applied": true }));
    let said = s
        .wait_for(Duration::from_secs(10), |m| {
            matches!(m, Message::Notification(n)
            if n.method == "window/showMessage")
        })
        .expect("a message");
    let Message::Notification(note) = said else {
        unreachable!()
    };
    assert_eq!(
        note.params["message"],
        "conjure: the editor did not report the undo, so Try harder was not started"
    );
    assert_eq!(s.script.seen().len(), 1);
    s.stop();
}

#[test]
fn an_open_file_back_at_its_text_before_does_not_refuse_the_undo() {
    let (_dir, s) = landed(&[&three_files()]);
    let main = s.marks("src/a.py");
    // the person put b.py back by hand
    s.type_text("src/b.py", B);
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Undo"));
    let params = s.apply_next_edit();
    let uris: Vec<String> = kinds_and_uris(&params["edit"])
        .into_iter()
        .map(|(_, uri, _)| uri)
        .collect();
    assert_eq!(uris, [s.uri("src/a.py"), s.uri("src/a__test.py")]);
    assert_eq!(s.text("src/a.py").unwrap(), A);
    s.stop();
}

#[test]
fn undo_deletes_an_empty_created_file_whose_buffer_was_closed() {
    let (_dir, s) = landed(&[&three_files()]);
    let main = s.marks("src/a.py");
    // closed unsaved: the editor created it empty on disk, and the text is gone
    s.close("src/a__test.py");
    s.run(&mark_action(&s, "src/a.py", &main, "conjure: Undo"));
    let params = s.apply_next_edit();
    let kinds: Vec<(String, String)> = kinds_and_uris(&params["edit"])
        .into_iter()
        .map(|(kind, uri, _)| (kind, uri))
        .collect();
    assert!(
        kinds.contains(&("delete".to_string(), s.uri("src/a__test.py"))),
        "{kinds:?}"
    );
    assert!(!s.path("src/a__test.py").exists());
    s.stop();
}
