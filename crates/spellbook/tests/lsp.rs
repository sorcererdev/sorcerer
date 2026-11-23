//! spellbook's language server, driven over an in-memory connection by a small editor that keeps
//! its own buffers and answers what the server asks it.
mod support;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::Diagnostic;
use serde_json::{Value, json};
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_lsp::uri_of;
use spellbook::adapters::lsp::{ServerConfig, serve};
use spellbook::ports::clock::Clock;
use support::{bistro_project, test_cache, vendored_project, write_file};
use tempfile::TempDir;

/// How long a test waits for something the server must send.
const WAIT: Duration = Duration::from_secs(20);
/// How long a test watches for something the server must not send.
const QUIET: Duration = Duration::from_millis(300);

/// A clock the test moves.
struct TestClock(Mutex<DateTime<Utc>>);

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

/// The editor's end of a server running on a thread.
struct Served {
    editor: Connection,
    root: PathBuf,
    server: Option<JoinHandle<()>>,
    inbox: RefCell<VecDeque<Message>>,
    requests_sent: Cell<i32>,
    clock: Arc<TestClock>,
    trust: TrustList,
    /// What the server answered to `initialize`.
    handshake: RefCell<Value>,
    _trust_home: TempDir,
}

fn served(dir: &Path) -> Served {
    let trust_home = TempDir::new().unwrap();
    let trust = TrustList::at(trust_home.path());
    let clock = Arc::new(TestClock(Mutex::new("2026-11-22T12:00:00Z".parse().unwrap())));
    let cfg = ServerConfig {
        open: Box::new(|root| Ok(vendored_project(root))),
        cache: test_cache(),
        trust: Some(trust.clone()),
        clock: clock.clone(),
    };
    let (server_end, editor_end) = Connection::memory();
    let server = std::thread::spawn(move || serve(server_end, cfg).unwrap());
    let root = canonical(dir).unwrap();
    let s = Served {
        editor: editor_end,
        root: root.clone(),
        server: Some(server),
        inbox: RefCell::new(VecDeque::new()),
        requests_sent: Cell::new(0),
        clock,
        trust,
        handshake: RefCell::new(Value::Null),
        _trust_home: trust_home,
    };
    let id = s.request(
        "initialize",
        json!({ "capabilities": { "window": { "showDocument": { "support": true } } },
                "rootUri": uri_of(&root).as_str() }),
    );
    *s.handshake.borrow_mut() = s.response(id).response_result.unwrap();
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
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": uri_of(&path).as_str(), "languageId": "", "version": 1, "text": text
            }}),
        );
    }

    /// The editor saves a file that was changed on the disk.
    fn save(&self, relative: &str) {
        self.notify(
            "textDocument/didSave",
            json!({ "textDocument": { "uri": self.uri(relative) } }),
        );
    }

    fn close(&self, relative: &str) {
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": self.uri(relative) } }),
        );
    }

    /// The next message from the server; a document to show is answered as a real editor would.
    fn pull(&self, deadline: Instant) -> Option<Message> {
        let left = deadline.saturating_duration_since(Instant::now());
        let message = self.editor.receiver.recv_timeout(left).ok()?;
        if let Message::Request(r) = &message
            && r.method == "window/showDocument"
        {
            self.reply(r.id.clone(), json!({ "success": true }));
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

    /// Runs a command and returns its reply.
    fn command(&self, command: &str, arguments: Value) -> Value {
        let id = self.request(
            "workspace/executeCommand",
            json!({ "command": command, "arguments": arguments }),
        );
        self.response(id).response_result.expect("a reply")
    }

    /// The error a command is refused with.
    fn command_error(&self, command: &str) -> String {
        let id = self.request(
            "workspace/executeCommand",
            json!({ "command": command, "arguments": [] }),
        );
        self.response(id).response_result.expect_err("an error").message
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

    /// The next document the server asked the editor to show.
    fn shown_document(&self) -> Value {
        let message = self
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

    fn stop(mut self) {
        let id = self.request("shutdown", Value::Null);
        self.response(id);
        self.notify("exit", Value::Null);
        self.server.take().unwrap().join().unwrap();
    }
}

fn is_publish_for(message: &Message, uri: &str) -> bool {
    matches!(message, Message::Notification(n)
        if n.method == "textDocument/publishDiagnostics" && n.params["uri"] == uri)
}

// ---- Task B1: the skeleton ----

const COMMANDS: [&str; 4] = [
    "spellbook.generators",
    "spellbook.choices",
    "spellbook.plan",
    "spellbook.apply",
];

#[test]
fn the_handshake_announces_the_four_commands_and_no_change_sync() {
    let dir = bistro_project();
    let s = served(dir.path());
    let caps = s.handshake.borrow()["capabilities"].clone();
    let mut commands: Vec<String> =
        serde_json::from_value(caps["executeCommandProvider"]["commands"].clone()).unwrap();
    commands.sort();
    let mut wanted: Vec<String> = COMMANDS.map(String::from).to_vec();
    wanted.sort();
    assert_eq!(commands, wanted);
    // no text sync of changes: open and close and save only
    let change = &caps["textDocumentSync"]["change"];
    assert!(change.is_null() || *change == json!(0), "{change}");
    assert_eq!(caps["textDocumentSync"]["openClose"], json!(true));
    assert!(caps.get("codeActionProvider").is_none());
    s.stop();
}

#[test]
fn an_unknown_command_is_refused_by_name() {
    let dir = bistro_project();
    let s = served(dir.path());
    assert!(
        s.command_error("spellbook.nothing")
            .contains("spellbook.nothing")
    );
    s.stop();
}

// ---- Task B2: generators, choices, plan ----

const MAIN: &str = "src/bistro/booking/commands/cancel_table.py";
const RUNNER: &str = "[model.providers.mine]\nrunner = \"/x/run\"\n";

#[test]
fn generators_lists_the_command_generator_with_its_inputs() {
    let dir = bistro_project();
    write_file(
        dir.path(),
        ".sorcerer/spellbook/generators/broken/generator.toml",
        "nonsense = [",
    );
    let s = served(dir.path());
    let reply = s.command("spellbook.generators", json!([]));
    let generators = reply.as_array().unwrap();
    // the broken one is left out
    assert_eq!(generators.len(), 1, "{reply}");
    assert_eq!(generators[0]["id"], "command");
    assert_eq!(generators[0]["name"], "Command");
    let inputs: Vec<(&str, &str)> = generators[0]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["name"].as_str().unwrap(), i["type"].as_str().unwrap()))
        .collect();
    assert_eq!(
        inputs,
        [("app", "enum"), ("service", "enum"), ("command", "ident")]
    );
    assert_eq!(generators[0]["inputs"][0]["optional"], false);
    s.stop();
}

#[test]
fn choices_for_service_after_app() {
    let dir = bistro_project();
    let s = served(dir.path());
    let reply = s.command(
        "spellbook.choices",
        json!(["command", { "app": "bistro" }, "service"]),
    );
    assert_eq!(reply, json!({ "choices": ["booking"] }));
    let reply = s.command("spellbook.choices", json!(["command", {}, "app"]));
    assert_eq!(reply, json!({ "choices": ["bistro"] }));
    // an empty table crosses as an empty list from Lua
    let reply = s.command("spellbook.choices", json!(["command", [], "app"]));
    assert_eq!(reply, json!({ "choices": ["bistro"] }));
    s.stop();
}

#[test]
fn choices_for_an_unknown_generator_is_a_stop() {
    let dir = bistro_project();
    let s = served(dir.path());
    let reply = s.command("spellbook.choices", json!(["nope", {}, "app"]));
    assert!(reply["stop"].as_str().unwrap().contains("no generator named nope"));
    s.stop();
}

fn given_all() -> Value {
    json!({ "app": "bistro", "service": "booking", "command": "cancel_table" })
}

#[test]
fn plan_returns_files_and_holes_and_writes_nothing() {
    let dir = bistro_project();
    let s = served(dir.path());
    let reply = s.command("spellbook.plan", json!(["command", given_all()]));
    assert!(reply["id"].as_str().unwrap().len() == 8, "{reply}");
    let files = reply["files"].as_array().unwrap();
    let statuses: Vec<(&str, &str)> = files
        .iter()
        .map(|f| (f["path"].as_str().unwrap(), f["status"].as_str().unwrap()))
        .collect();
    assert_eq!(statuses[0].1, "skipped");
    assert_eq!(statuses[1], (MAIN, "new"));
    assert!(files[1]["text"].as_str().unwrap().contains("NotImplementedError"));
    assert!(files[1]["lines"].as_u64().unwrap() > 5);
    let holes = reply["holes"].as_array().unwrap();
    let handle = holes.iter().find(|h| h["name"] == "handle").unwrap();
    assert_eq!(handle["kind"], "block");
    assert_eq!(handle["source"], "placeholder");
    assert_eq!(handle["places"][0]["file"], MAIN);
    assert!(handle["places"][0]["line"].as_u64().unwrap() > 0);
    assert!(!dir.path().join(MAIN).exists());
    s.stop();
}

#[test]
fn plan_with_a_missing_input_returns_the_stop_and_choices() {
    let dir = bistro_project();
    write_file(dir.path(), "src/bistro/kitchen/models/__init__.py", "");
    let s = served(dir.path());
    let reply = s.command(
        "spellbook.plan",
        json!(["command", { "app": "bistro", "command": "cancel_table" }]),
    );
    assert!(reply["stop"].as_str().unwrap().contains("service"), "{reply}");
    assert_eq!(reply["input"], "service");
    assert_eq!(reply["choices"], json!(["booking", "kitchen"]));
    s.stop();
}

#[test]
fn an_untrusted_project_is_refused() {
    let dir = bistro_project();
    write_file(dir.path(), ".sorcerer/config.toml", RUNNER);
    let s = served(dir.path());
    let sentence = "spellbook: this project is not trusted; run \"spellbook trust\" in it";
    for (command, arguments) in [
        ("spellbook.generators", json!([])),
        ("spellbook.choices", json!(["command", {}, "app"])),
        ("spellbook.plan", json!(["command", given_all()])),
        ("spellbook.apply", json!(["abcd1234"])),
    ] {
        assert_eq!(s.command(command, arguments), json!({ "stop": sentence }));
    }
    s.trust.trust(&s.root).unwrap();
    assert!(s.command("spellbook.generators", json!([])).is_array());
    s.stop();
}
