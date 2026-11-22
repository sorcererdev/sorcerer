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
}

impl Default for Setup {
    fn default() -> Setup {
        Setup {
            capabilities: full_capabilities(),
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
        json!({ "capabilities": setup.capabilities, "rootUri": uri_of(&root).as_str() }),
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

    fn version(&self, relative: &str) -> i32 {
        self.buffers.borrow()[&self.path(relative)].0
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
        let changes = edit["documentChanges"].as_array().unwrap();
        // every version checked before anything changes, as an editor applies it whole
        for change in changes {
            if let Some(version) = change["textDocument"]["version"].as_i64() {
                let path = path_of(&change["textDocument"]["uri"]);
                let have = self.buffers.borrow().get(&path).map(|(v, _)| i64::from(*v));
                if have != Some(version) {
                    return false;
                }
            }
        }
        for change in changes {
            match change["kind"].as_str() {
                Some("create") => {
                    let path = path_of(&change["uri"]);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, "").unwrap();
                    self.opened_with(&path, 1, "");
                }
                Some("delete") => {
                    let path = path_of(&change["uri"]);
                    let _ = std::fs::remove_file(&path);
                    self.buffers.borrow_mut().remove(&path);
                    self.notify(
                        "textDocument/didClose",
                        json!({ "textDocument": { "uri": uri_of(&path).as_str() } }),
                    );
                }
                _ => {
                    let path = path_of(&change["textDocument"]["uri"]);
                    if !self.buffers.borrow().contains_key(&path) {
                        let text = std::fs::read_to_string(&path).unwrap();
                        self.opened_with(&path, 1, &text);
                    }
                    let (version, text) = self.buffers.borrow()[&path].clone();
                    let edits = change["edits"].as_array().unwrap();
                    self.changed_to(&path, version + 1, &applied_edits(&text, edits));
                }
            }
        }
        true
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

    /// Waits until the records hold N lines.
    fn records_reach(&self, n: usize) -> Vec<ChangeRecord> {
        let deadline = Instant::now() + WAIT;
        loop {
            let records = self.records();
            if records.len() >= n || Instant::now() > deadline {
                return records;
            }
            // the server writes from the main loop; let it run
            let _ = self.all_within(Duration::from_millis(20));
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
