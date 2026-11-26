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
use support::{
    CLOSE_TABLE_PATH, CLOSE_TABLE_TEST_PATH, bistro_project, bistro_rs_project,
    bistro_rs_with_regions, bistro_with_the_fix, test_cache, use_corpus_rules, vendored_project,
    write_file,
};
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
    served_with(
        dir,
        json!({ "window": { "showDocument": { "support": true } } }),
    )
}

/// A server for an editor that applies the edits the server sends, tied to versions.
fn served_editing(dir: &Path) -> Served {
    served_with(
        dir,
        json!({ "window": { "showDocument": { "support": true } },
                "workspace": { "applyEdit": true,
                               "workspaceEdit": { "documentChanges": true } } }),
    )
}

fn served_with(dir: &Path, capabilities: Value) -> Served {
    let trust_home = TempDir::new().unwrap();
    let trust = TrustList::at(trust_home.path());
    let clock = Arc::new(TestClock(Mutex::new(
        "2026-11-22T12:00:00Z".parse().unwrap(),
    )));
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
        json!({ "capabilities": capabilities, "rootUri": uri_of(&root).as_str() }),
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
        self.response(id)
            .response_result
            .expect_err("an error")
            .message
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
fn the_handshake_announces_the_four_commands_code_actions_and_no_change_sync() {
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
    assert_eq!(caps["codeActionProvider"], json!(true));
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
    assert!(
        reply["stop"]
            .as_str()
            .unwrap()
            .contains("no generator named nope")
    );
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
    assert!(
        files[1]["text"]
            .as_str()
            .unwrap()
            .contains("NotImplementedError")
    );
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
    assert!(
        reply["stop"].as_str().unwrap().contains("service"),
        "{reply}"
    );
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

// ---- Task B3: apply and the hole marks ----

const TEST_FILE: &str = "src/bistro/booking/commands/cancel_table__test.py";
const GONE: &str = "that plan is gone (plans last ten minutes); make it again";

fn planned(s: &Served) -> String {
    let plan = s.command("spellbook.plan", json!(["command", given_all()]));
    plan["id"].as_str().unwrap().to_string()
}

fn message_of(mark: &Diagnostic, name: &str) -> bool {
    mark.message.starts_with(&format!("{name} ("))
}

/// The text a range covers on one line of the file.
fn slice_of(text: &str, range: &lsp_types::Range) -> String {
    let row = text.lines().nth(range.start.line as usize).unwrap();
    let units: Vec<u16> = row.encode_utf16().collect();
    String::from_utf16(&units[range.start.character as usize..range.end.character as usize])
        .unwrap()
}

#[test]
fn apply_reply_lists_the_stops_in_order() {
    let dir = bistro_project();
    let s = served(dir.path());
    let id = planned(&s);
    let reply = s.command("spellbook.apply", json!([id]));
    let stops = reply["stops"].as_array().unwrap();
    let named: Vec<(&str, &str)> = stops
        .iter()
        .map(|stop| {
            (
                stop["file"].as_str().unwrap(),
                stop["name"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        named,
        [
            (MAIN, "field"),
            (MAIN, "type"),
            (MAIN, "handle"),
            (TEST_FILE, "field")
        ]
    );
    for stop in stops {
        let range: lsp_types::Range = serde_json::from_value(stop["range"].clone()).unwrap();
        let text =
            std::fs::read_to_string(dir.path().join(stop["file"].as_str().unwrap())).unwrap();
        assert!(!slice_of(&text, &range).is_empty(), "{stop}");
        assert!(stop["kind"].is_string() && stop["instruction"].is_string());
    }
    let main_text = std::fs::read_to_string(dir.path().join(MAIN)).unwrap();
    let first: lsp_types::Range = serde_json::from_value(stops[0]["range"].clone()).unwrap();
    assert_eq!(slice_of(&main_text, &first), "todo_field");
    assert_eq!(reply["first"]["file"], MAIN);
    assert_eq!(reply["first"]["line"], first.start.line + 1);
    let shown = s.shown_document();
    assert_eq!(shown["selection"], stops[0]["range"]);
    s.stop();
}

#[test]
fn apply_writes_publishes_marks_and_shows_the_first_hole() {
    let dir = bistro_project();
    let s = served(dir.path());
    let id = planned(&s);
    let reply = s.command("spellbook.apply", json!([id]));
    assert_eq!(reply["written"], json!([MAIN, TEST_FILE]));
    assert_eq!(
        reply["skipped"],
        json!(["src/bistro/booking/commands/__init__.py"])
    );
    assert!(reply["refused"].is_null(), "{reply}");
    assert!(dir.path().join(MAIN).exists());
    assert!(dir.path().join(TEST_FILE).exists());
    assert_eq!(reply["holes"].as_array().unwrap().len(), 6);
    let first_line = reply["first"]["line"].as_u64().unwrap();
    assert_eq!(reply["first"]["file"], MAIN);

    let shown = s.shown_document();
    assert_eq!(shown["uri"], s.uri(MAIN));
    assert_eq!(shown["selection"]["start"]["line"], first_line - 1);

    let marks = s.marks(MAIN);
    let names: Vec<&str> = marks
        .iter()
        .map(|m| m.message.split(' ').next().unwrap())
        .collect();
    assert_eq!(names, ["field", "type", "handle"]);
    let handle = marks.iter().find(|m| message_of(m, "handle")).unwrap();
    assert_eq!(
        handle.message,
        "handle (block): its first case, from the test file beside it"
    );
    assert_eq!(handle.source.as_deref(), Some("spellbook"));
    assert_eq!(
        handle.code,
        Some(lsp_types::NumberOrString::String("hole".into()))
    );
    assert_eq!(
        handle.severity,
        Some(lsp_types::DiagnosticSeverity::INFORMATION)
    );
    let on_disk = std::fs::read_to_string(dir.path().join(MAIN)).unwrap();
    assert!(
        slice_of(&on_disk, &handle.range).starts_with("raise NotImplementedError(\"hole handle:"),
        "{on_disk}"
    );
    let field = marks.iter().find(|m| message_of(m, "field")).unwrap();
    assert_eq!(slice_of(&on_disk, &field.range), "todo_field");
    let test_marks = s.marks(TEST_FILE);
    assert_eq!(test_marks.len(), 1);
    assert!(message_of(&test_marks[0], "field"));
    s.stop();
}

#[test]
fn apply_of_an_unknown_plan_says_it_is_gone() {
    let dir = bistro_project();
    let s = served(dir.path());
    assert_eq!(
        s.command("spellbook.apply", json!(["deadbeef"])),
        json!({ "stop": GONE })
    );
    s.stop();
}

#[test]
fn a_plan_is_applied_once() {
    let dir = bistro_project();
    let s = served(dir.path());
    let id = planned(&s);
    assert!(s.command("spellbook.apply", json!([id]))["written"].is_array());
    assert_eq!(
        s.command("spellbook.apply", json!([id])),
        json!({ "stop": GONE })
    );
    s.stop();
}

#[test]
fn a_plan_expires_after_ten_minutes() {
    let dir = bistro_project();
    let s = served(dir.path());
    let id = planned(&s);
    *s.clock.0.lock().unwrap() = "2026-11-22T12:11:00Z".parse().unwrap();
    assert_eq!(
        s.command("spellbook.apply", json!([id])),
        json!({ "stop": GONE })
    );
    assert!(!dir.path().join(MAIN).exists());
    s.stop();
}

#[test]
fn a_file_that_appeared_after_the_plan_is_reported_refused() {
    let dir = bistro_project();
    let s = served(dir.path());
    let id = planned(&s);
    write_file(dir.path(), MAIN, "# mine\n");
    let reply = s.command("spellbook.apply", json!([id]));
    assert_eq!(reply["refused"]["path"], MAIN);
    assert!(reply["refused"]["why"].is_string());
    assert_eq!(reply["written"], json!([]));
    assert!(reply["first"].is_null());
    s.stop();
}

// ---- Task B4: marks follow the disk on open and save ----

/// Applies the plan and takes the marks the apply published, so the next ones are the test's.
fn applied(s: &Served) {
    let id = planned(s);
    s.command("spellbook.apply", json!([id]));
    s.marks(MAIN);
    s.marks(TEST_FILE);
}

fn fill_handle_on_disk(dir: &Path) {
    let path = dir.join(MAIN);
    let text = std::fs::read_to_string(&path).unwrap();
    let filled: Vec<String> = text
        .lines()
        .map(|line| {
            if line.contains("hole handle") {
                "        return Ok(None)".to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    std::fs::write(path, filled.join("\n") + "\n").unwrap();
}

#[test]
fn opening_a_generated_file_shows_its_marks() {
    let dir = bistro_project();
    let s = served(dir.path());
    applied(&s);
    s.open(MAIN);
    assert_eq!(s.marks(MAIN).len(), 3);
    s.stop();
}

#[test]
fn saving_a_file_whose_placeholder_was_replaced_drops_its_mark() {
    let dir = bistro_project();
    let s = served(dir.path());
    applied(&s);
    s.open(MAIN);
    s.marks(MAIN);
    fill_handle_on_disk(dir.path());
    s.save(MAIN);
    let marks = s.marks(MAIN);
    let names: Vec<&str> = marks
        .iter()
        .map(|m| m.message.split(' ').next().unwrap())
        .collect();
    assert_eq!(names, ["field", "type"]);
    s.stop();
}

#[test]
fn closing_clears_the_marks() {
    let dir = bistro_project();
    let s = served(dir.path());
    applied(&s);
    s.open(MAIN);
    s.marks(MAIN);
    s.close(MAIN);
    assert!(s.marks(MAIN).is_empty());
    s.stop();
}

#[test]
fn a_file_outside_the_project_gets_nothing() {
    let dir = bistro_project();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("x.py"), "x = 1\n").unwrap();
    let s = served(dir.path());
    s.notify(
        "textDocument/didOpen",
        json!({ "textDocument": {
            "uri": uri_of(&canonical(&elsewhere.path().join("x.py")).unwrap()).as_str(),
            "languageId": "", "version": 1, "text": "x = 1\n" }}),
    );
    let seen = s.all_within(QUIET);
    assert!(
        !seen.iter().any(|m| matches!(m, Message::Notification(n)
            if n.method == "textDocument/publishDiagnostics")),
        "{seen:?}"
    );
    s.stop();
}

#[test]
fn an_untrusted_project_publishes_no_marks() {
    let dir = bistro_project();
    let s = served(dir.path());
    applied(&s);
    write_file(dir.path(), ".sorcerer/config.toml", RUNNER);
    s.open(MAIN);
    let seen = s.all_within(QUIET);
    assert!(
        !seen.iter().any(|m| matches!(m, Message::Notification(n)
            if n.method == "textDocument/publishDiagnostics")),
        "{seen:?}"
    );
    s.stop();
}

#[test]
fn an_untrusted_project_says_so_once_on_open_and_save() {
    let dir = bistro_project();
    write_file(dir.path(), ".sorcerer/config.toml", RUNNER);
    let s = served(dir.path());
    s.open(MAIN);
    s.save(MAIN);
    let said: Vec<String> = s
        .all_within(QUIET)
        .into_iter()
        .filter_map(|m| match m {
            Message::Notification(n) if n.method == "window/showMessage" => {
                n.params["message"].as_str().map(String::from)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        said,
        ["spellbook: this project is not trusted; run \"spellbook trust\" in it"]
    );
    s.stop();
}

#[test]
fn spellbook_off_in_the_config_refuses_commands_and_shows_no_marks() {
    let dir = bistro_project();
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook]\nenabled = false\n",
    );
    let s = served(dir.path());
    let sentence = "spellbook is off in this project ([spellbook] enabled = false)";
    assert_eq!(
        s.command("spellbook.generators", json!([])),
        json!({ "stop": sentence })
    );
    s.open(MAIN);
    let seen = s.all_within(QUIET);
    assert!(
        !seen.iter().any(|m| matches!(m, Message::Notification(n)
            if n.method == "textDocument/publishDiagnostics")),
        "{seen:?}"
    );
    s.stop();
}

// ---- Task C8: findings ----

const CLOSE: &str = "src/bistro/booking/commands/close_table.py";

fn code_of(mark: &Diagnostic) -> String {
    match mark.code.as_ref() {
        Some(lsp_types::NumberOrString::String(code)) => code.clone(),
        other => format!("{other:?}"),
    }
}

#[test]
fn saving_a_command_with_sqlite3_shows_an_error() {
    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    write_file(dir.path(), CLOSE, "from dataclasses import dataclass\n");
    let s = served(dir.path());
    s.open(CLOSE);
    assert!(s.marks(CLOSE).is_empty());
    write_file(
        dir.path(),
        CLOSE,
        "from dataclasses import dataclass\nimport sqlite3\n",
    );
    s.save(CLOSE);
    let marks = s.marks(CLOSE);
    assert_eq!(marks.len(), 1);
    let finding = &marks[0];
    assert_eq!(finding.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
    assert_eq!(code_of(finding), "commands-use-ports");
    assert_eq!(finding.source.as_deref(), Some("spellbook"));
    assert_eq!(finding.range.start.line, 1);
    assert!(
        finding
            .message
            .starts_with("Commands and queries talk to ports")
    );
    // an allow with a reason clears it
    write_file(
        dir.path(),
        CLOSE,
        "import sqlite3  # spellbook:allow commands-use-ports — migration\n",
    );
    s.save(CLOSE);
    assert!(s.marks(CLOSE).is_empty());
    s.stop();
}

#[test]
fn marks_and_findings_are_published_together() {
    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    let s = served(dir.path());
    applied(&s);
    let text = std::fs::read_to_string(dir.path().join(MAIN)).unwrap();
    std::fs::write(
        dir.path().join(MAIN),
        text.replacen(
            "from dataclasses import dataclass\n",
            "from dataclasses import dataclass\nimport sqlite3\n",
            1,
        ),
    )
    .unwrap();
    s.open(MAIN);
    let marks = s.marks(MAIN);
    let codes: Vec<String> = marks.iter().map(code_of).collect();
    assert_eq!(
        codes.iter().filter(|c| *c == "hole").count(),
        3,
        "{codes:?}"
    );
    assert_eq!(
        codes.iter().filter(|c| *c == "commands-use-ports").count(),
        1,
        "{codes:?}"
    );
    // the finding is not what the first hole is
    s.stop();
}

#[test]
fn a_broken_rules_file_is_said_once_and_findings_are_left_out() {
    let dir = bistro_project();
    write_file(
        dir.path(),
        ".sorcerer/spellbook/rules.toml",
        "[rules.a]\nfiles = \"x\"\n",
    );
    write_file(dir.path(), CLOSE, "import sqlite3\n");
    let s = served(dir.path());
    s.open(CLOSE);
    assert!(s.marks(CLOSE).is_empty());
    s.save(CLOSE);
    s.marks(CLOSE);
    let said: Vec<Message> = s
        .all_within(QUIET)
        .into_iter()
        .filter(|m| matches!(m, Message::Notification(n) if n.method == "window/showMessage"))
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    let Message::Notification(note) = &said[0] else {
        unreachable!()
    };
    assert!(
        note.params["message"]
            .as_str()
            .unwrap()
            .contains("rule a: `message` is missing"),
        "{note:?}"
    );
    s.stop();
}

// ---- M2a Part B1: a finding's fixes as code actions ----

/// The code actions the server offers on one line (from 0) of a file.
fn code_actions(s: &Served, relative: &str, row: u32) -> Vec<Value> {
    let id = s.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": s.uri(relative) },
            "range": { "start": { "line": row, "character": 0 },
                       "end": { "line": row, "character": 0 } },
            "context": { "diagnostics": [] },
        }),
    );
    let reply = s.response(id).response_result.expect("a reply");
    serde_json::from_value(reply).unwrap_or_default()
}

/// The fix fixture served, the command file opened and its findings published.
fn served_with_the_finding(dir: &Path) -> Served {
    let s = served(dir);
    s.open(CLOSE_TABLE_PATH);
    s.marks(CLOSE_TABLE_PATH);
    s
}

#[test]
fn a_finding_offers_its_fixes_as_quick_fixes_the_first_preferred() {
    let dir = bistro_with_the_fix();
    let s = served_with_the_finding(dir.path());
    let actions = code_actions(&s, CLOSE_TABLE_PATH, 3);
    let titles: Vec<&str> = actions
        .iter()
        .map(|action| action["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        [
            "spellbook: Use a port the service has",
            "spellbook: Add a port and use it"
        ]
    );
    let first = &actions[0];
    assert_eq!(first["kind"], "quickfix");
    assert_eq!(first["isPreferred"], true);
    assert_ne!(actions[1]["isPreferred"], true);
    assert_eq!(first["diagnostics"][0]["code"], "commands-use-ports");
    assert_eq!(first["diagnostics"][0]["range"]["start"]["line"], 3);
    assert_eq!(first["command"]["command"], "spellbook.fix");
    assert_eq!(
        first["command"]["arguments"],
        json!([{
            "generator": "use-port",
            "inputs": { "app": "bistro", "service": "booking", "kind": "commands",
                        "unit": "close_table" },
            "missing": ["port"],
            "fix": { "file": CLOSE_TABLE_PATH, "line": 4, "rule": "commands-use-ports",
                     "index": 1, "label": "Use a port the service has", "version": null },
        }])
    );
    assert_eq!(actions[1]["command"]["arguments"][0]["fix"]["index"], 2);
    s.stop();
}

#[test]
fn a_line_without_a_finding_has_no_actions() {
    let dir = bistro_with_the_fix();
    let s = served_with_the_finding(dir.path());
    assert!(code_actions(&s, CLOSE_TABLE_PATH, 0).is_empty());
    s.stop();
}

#[test]
fn only_other_kinds_asked_for_gets_no_quick_fixes() {
    let dir = bistro_with_the_fix();
    let s = served_with_the_finding(dir.path());
    let id = s.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": s.uri(CLOSE_TABLE_PATH) },
            "range": { "start": { "line": 3, "character": 0 },
                       "end": { "line": 3, "character": 0 } },
            "context": { "diagnostics": [], "only": ["refactor"] },
        }),
    );
    assert_eq!(s.response(id).response_result.unwrap(), json!([]));
    s.stop();
}

#[test]
fn an_untrusted_project_offers_no_fixes() {
    let dir = bistro_with_the_fix();
    write_file(dir.path(), ".sorcerer/config.toml", RUNNER);
    let s = served(dir.path());
    s.open(CLOSE_TABLE_PATH);
    assert!(code_actions(&s, CLOSE_TABLE_PATH, 3).is_empty());
    s.stop();
}

// ---- M2a Part B2: a fix planned ----

/// The use-port fix of the finding on `close_table.py:4`, as the editor sends it to `plan`.
fn use_port_fix(version: Value) -> Value {
    json!({ "file": CLOSE_TABLE_PATH, "line": 4, "rule": "commands-use-ports", "index": 1,
            "version": version })
}

fn use_port_inputs() -> Value {
    json!({ "app": "bistro", "service": "booking", "kind": "commands", "unit": "close_table",
            "port": "table_repository" })
}

fn plan_use_port(s: &Served, version: Value) -> Value {
    s.command(
        "spellbook.plan",
        json!(["use-port", use_port_inputs(), use_port_fix(version)]),
    )
}

#[test]
fn plan_with_a_fix_shows_the_changed_files_and_writes_nothing() {
    let dir = bistro_with_the_fix();
    let before = std::fs::read_to_string(dir.path().join(CLOSE_TABLE_PATH)).unwrap();
    let s = served(dir.path());
    let reply = plan_use_port(&s, json!(1));
    let files = reply["files"]
        .as_array()
        .unwrap_or_else(|| panic!("{reply}"));
    let rows: Vec<(&str, &str)> = files
        .iter()
        .map(|f| (f["path"].as_str().unwrap(), f["status"].as_str().unwrap()))
        .collect();
    assert_eq!(
        rows,
        [
            (CLOSE_TABLE_PATH, "changed"),
            (CLOSE_TABLE_TEST_PATH, "changed")
        ]
    );
    let module = &files[0];
    assert!(module["added"].as_u64().unwrap() > 0, "{module}");
    assert!(module["removed"].as_u64().unwrap() > 0, "{module}");
    let hunk = &module["hunks"][0];
    assert!(hunk["from"].as_u64().unwrap() >= 1, "{hunk}");
    assert!(hunk["to"].as_u64().unwrap() >= hunk["from"].as_u64().unwrap());
    assert!(
        hunk["lines"]
            .as_array()
            .unwrap()
            .contains(&json!("-import sqlite3")),
        "{hunk}"
    );
    assert_eq!(reply["fix"]["rule"], "commands-use-ports");
    assert_eq!(reply["fix"]["label"], "Use a port the service has");
    assert_eq!(reply["fix"]["file"], CLOSE_TABLE_PATH);
    assert_eq!(reply["fix"]["line"], 4);
    assert_eq!(
        std::fs::read_to_string(dir.path().join(CLOSE_TABLE_PATH)).unwrap(),
        before
    );
    s.stop();
}

#[test]
fn plan_with_a_fix_whose_finding_is_gone_is_a_stop() {
    let dir = bistro_with_the_fix();
    let s = served(dir.path());
    let reply = s.command(
        "spellbook.plan",
        json!(["use-port", use_port_inputs(),
               { "file": CLOSE_TABLE_PATH, "line": 1, "rule": "commands-use-ports",
                 "index": 1 }]),
    );
    assert_eq!(
        reply,
        json!({ "stop": format!(
            "{CLOSE_TABLE_PATH}:1 has no commands-use-ports finding any more; nothing to fix (run `spellbook check`)"
        ) })
    );
    s.stop();
}

#[test]
fn plan_with_a_fix_already_applied_has_nothing_to_do() {
    let dir = bistro_with_the_fix();
    let s = served(dir.path());
    let reply = plan_use_port(&s, Value::Null);
    for file in reply["files"].as_array().unwrap() {
        write_file(
            dir.path(),
            file["path"].as_str().unwrap(),
            file["text"].as_str().unwrap(),
        );
    }
    assert_eq!(
        plan_use_port(&s, Value::Null),
        json!({ "stop": "nothing to do: the fix is already applied" })
    );
    s.stop();
}

#[test]
fn a_malformed_fix_argument_is_refused() {
    let dir = bistro_with_the_fix();
    let s = served(dir.path());
    let id = s.request(
        "workspace/executeCommand",
        json!({ "command": "spellbook.plan",
                "arguments": ["use-port", use_port_inputs(), { "file": CLOSE_TABLE_PATH }] }),
    );
    let error = s.response(id).response_result.expect_err("an error");
    assert!(error.message.contains("fix"), "{}", error.message);
    s.stop();
}

// ---- M2a Part B3: a fix applied ----

const CHANGED_SINCE: &str = "close_table.py changed since the plan; nothing was applied: fix again";
const CANNOT_EDIT: &str =
    "spellbook: your editor cannot apply edits; changed files were written to disk";

/// The add-port fix (four new files, two changed) of the finding on `close_table.py:4`.
fn plan_add_port(s: &Served, version: Value) -> Value {
    let mut inputs = use_port_inputs();
    inputs["port"] = json!("clock");
    s.command(
        "spellbook.plan",
        json!(["add-port", inputs,
               { "file": CLOSE_TABLE_PATH, "line": 4, "rule": "commands-use-ports", "index": 2,
                 "version": version }]),
    )
}

fn paths_with_status(plan: &Value, status: &str) -> Vec<String> {
    plan["files"]
        .as_array()
        .unwrap_or_else(|| panic!("{plan}"))
        .iter()
        .filter(|file| file["status"] == status)
        .map(|file| file["path"].as_str().unwrap().to_string())
        .collect()
}

fn planned_text(plan: &Value, path: &str) -> String {
    let file = plan["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == path)
        .unwrap();
    file["text"].as_str().unwrap().to_string()
}

/// Asks for the apply without waiting for its reply.
fn apply_asked(s: &Served, plan: &Value) -> i32 {
    s.request(
        "workspace/executeCommand",
        json!({ "command": "spellbook.apply", "arguments": [plan["id"]] }),
    )
}

/// The edit the server asked the editor to apply, not answered yet.
fn edit_asked(s: &Served) -> Request {
    match s.wait_for(
        WAIT,
        |m| matches!(m, Message::Request(r) if r.method == "workspace/applyEdit"),
    ) {
        Some(Message::Request(request)) => request,
        _ => panic!("no edit asked"),
    }
}

/// The text with a document's text edits made, as an editor makes them (every range is in the
/// text as it was; the test texts are ASCII, so a UTF-16 column is a byte).
fn with_edits_made(text: &str, edits: &[Value]) -> String {
    let offset = |position: &Value| {
        let line = position["line"].as_u64().unwrap() as usize;
        let column = position["character"].as_u64().unwrap() as usize;
        text.split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>()
            + column
    };
    let mut ranged: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|edit| {
            (
                offset(&edit["range"]["start"]),
                offset(&edit["range"]["end"]),
                edit["newText"].as_str().unwrap(),
            )
        })
        .collect();
    ranged.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut made = text.to_string();
    for (start, end, new_text) in ranged {
        made.replace_range(start..end, new_text);
    }
    made
}

fn findings_of(marks: &[Diagnostic]) -> usize {
    marks
        .iter()
        .filter(|mark| code_of(mark) == "commands-use-ports")
        .count()
}

#[test]
fn a_fix_lands_as_one_edit_and_its_new_files_are_written_after_the_editors_yes() {
    let dir = bistro_with_the_fix();
    let s = served_editing(dir.path());
    s.open(CLOSE_TABLE_PATH);
    let findings_before = findings_of(&s.marks(CLOSE_TABLE_PATH));
    let on_disk = |path: &str| std::fs::read_to_string(dir.path().join(path)).unwrap();
    let module_before = on_disk(CLOSE_TABLE_PATH);
    let plan = plan_add_port(&s, json!(7));
    let new_files = paths_with_status(&plan, "new");
    assert!(!new_files.is_empty(), "{plan}");
    assert_eq!(
        paths_with_status(&plan, "changed"),
        [CLOSE_TABLE_PATH, CLOSE_TABLE_TEST_PATH]
    );
    let id = apply_asked(&s, &plan);
    let asked = edit_asked(&s);
    assert_eq!(asked.params["label"], "spellbook: Add a port and use it");
    let documents = asked.params["edit"]["documentChanges"].as_array().unwrap();
    assert_eq!(
        documents[0]["textDocument"],
        json!({ "uri": s.uri(CLOSE_TABLE_PATH), "version": 7 })
    );
    assert_eq!(
        documents[1]["textDocument"],
        json!({ "uri": s.uri(CLOSE_TABLE_TEST_PATH), "version": null })
    );
    for (document, path) in documents
        .iter()
        .zip([CLOSE_TABLE_PATH, CLOSE_TABLE_TEST_PATH])
    {
        let edits = document["edits"].as_array().unwrap();
        assert_eq!(
            with_edits_made(&on_disk(path), edits),
            planned_text(&plan, path)
        );
    }
    // nothing is written, and apply is not answered, before the editor says yes
    let answered = RequestId::from(id);
    assert!(
        s.wait_for(
            QUIET,
            |m| matches!(m, Message::Response(r) if r.id == answered)
        )
        .is_none()
    );
    for path in &new_files {
        assert!(!dir.path().join(path).exists(), "{path}");
    }
    s.reply(asked.id, json!({ "applied": true }));
    let reply = s.response(id).response_result.unwrap();
    assert_eq!(
        reply["changed"],
        json!([CLOSE_TABLE_PATH, CLOSE_TABLE_TEST_PATH])
    );
    assert_eq!(reply["written"], json!(new_files));
    for path in &new_files {
        assert!(dir.path().join(path).exists(), "{path}");
    }
    // the changed files are the editor's buffers to save; the disk is not touched
    assert_eq!(on_disk(CLOSE_TABLE_PATH), module_before);
    // the finding the fix answered is gone from the marks, read from the planned text
    assert_eq!(findings_of(&s.marks(CLOSE_TABLE_PATH)), findings_before - 1);
    s.stop();
}

#[test]
fn an_edit_the_editor_refuses_writes_nothing_and_says_to_fix_again() {
    let dir = bistro_with_the_fix();
    let s = served_editing(dir.path());
    let plan = plan_add_port(&s, json!(3));
    let id = apply_asked(&s, &plan);
    let asked = edit_asked(&s);
    s.reply(
        asked.id,
        json!({ "applied": false, "failureReason": "version moved" }),
    );
    assert_eq!(
        s.response(id).response_result.unwrap(),
        json!({ "stop": CHANGED_SINCE })
    );
    for path in paths_with_status(&plan, "new") {
        assert!(!dir.path().join(&path).exists(), "{path}");
    }
    assert_eq!(
        s.command("spellbook.apply", json!([plan["id"]])),
        json!({ "stop": GONE })
    );
    s.stop();
}

#[test]
fn an_editor_that_cannot_apply_edits_gets_the_changes_on_disk_said_once() {
    let dir = bistro_with_the_fix();
    let s = served(dir.path());
    let plan = plan_use_port(&s, json!(1));
    let reply = s.command("spellbook.apply", json!([plan["id"]]));
    assert_eq!(
        reply["changed"],
        json!([CLOSE_TABLE_PATH, CLOSE_TABLE_TEST_PATH])
    );
    for path in [CLOSE_TABLE_PATH, CLOSE_TABLE_TEST_PATH] {
        assert_eq!(
            std::fs::read_to_string(dir.path().join(path)).unwrap(),
            planned_text(&plan, path)
        );
    }
    let said: Vec<Message> = s
        .all_within(QUIET)
        .into_iter()
        .filter(|m| {
            matches!(m, Message::Notification(n)
            if n.method == "window/showMessage" && n.params["message"] == CANNOT_EDIT)
        })
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    s.stop();
}

#[test]
fn a_file_edited_on_disk_after_the_plan_stops_the_apply() {
    let dir = bistro_with_the_fix();
    let s = served(dir.path());
    let plan = plan_use_port(&s, json!(1));
    let path = dir.path().join(CLOSE_TABLE_PATH);
    let edited = std::fs::read_to_string(&path).unwrap() + "# mine\n";
    std::fs::write(&path, &edited).unwrap();
    assert_eq!(
        s.command("spellbook.apply", json!([plan["id"]])),
        json!({ "stop": CHANGED_SINCE })
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    s.stop();
}

// ---- M2a Part B4: a hole in a changed file ----

#[test]
fn a_hole_the_fix_leaves_in_a_changed_file_is_marked_and_shown_before_the_save() {
    let dir = bistro_with_the_fix();
    let s = served_editing(dir.path());
    let text = std::fs::read_to_string(dir.path().join(CLOSE_TABLE_PATH)).unwrap();
    let connect_line = text
        .lines()
        .position(|line| line.contains("sqlite3.connect"))
        .unwrap()
        + 1;
    let plan = s.command(
        "spellbook.plan",
        json!(["use-port", use_port_inputs(),
               { "file": CLOSE_TABLE_PATH, "line": connect_line, "rule": "commands-use-ports",
                 "index": 1, "version": 2 }]),
    );
    assert_eq!(plan["holes"].as_array().map(Vec::len), Some(1), "{plan}");
    let id = apply_asked(&s, &plan);
    let asked = edit_asked(&s);
    s.reply(asked.id, json!({ "applied": true }));
    let reply = s.response(id).response_result.unwrap();
    let stops = reply["stops"].as_array().unwrap();
    assert_eq!(stops.len(), 1, "{reply}");
    assert_eq!(stops[0]["file"], CLOSE_TABLE_PATH);
    let range: lsp_types::Range = serde_json::from_value(stops[0]["range"].clone()).unwrap();
    let planned = planned_text(&plan, CLOSE_TABLE_PATH);
    assert!(slice_of(&planned, &range).contains("hole"), "{reply}");
    assert_eq!(s.shown_document()["selection"], stops[0]["range"]);
    let marks = s.marks(CLOSE_TABLE_PATH);
    assert_eq!(
        marks.iter().filter(|mark| code_of(mark) == "hole").count(),
        1
    );
    s.stop();
}

// ---- the editor opens a changed file while the fix lands ----

/// The marks the server publishes for a file the editor opens now, after what came before is
/// read and dropped.
fn marks_on_open(s: &Served, relative: &str) -> Vec<Diagnostic> {
    s.all_within(QUIET);
    s.open(relative);
    s.marks(relative)
}

#[test]
fn a_changed_file_opened_while_the_fix_lands_is_marked_from_the_plan_not_the_disk() {
    let dir = bistro_with_the_fix();
    let s = served_editing(dir.path());
    let on_disk = findings_of(&marks_on_open(&s, CLOSE_TABLE_PATH));
    let plan = plan_use_port(&s, json!(4));
    let id = apply_asked(&s, &plan);
    let asked = edit_asked(&s);
    // the editor opens the file to land the edit in it, before it answers: the import is gone
    assert_eq!(
        findings_of(&marks_on_open(&s, CLOSE_TABLE_PATH)),
        on_disk - 1
    );
    s.reply(asked.id, json!({ "applied": true }));
    s.response(id);
    // opened again after the edit landed, before the save: the disk still has `import sqlite3`
    assert_eq!(
        findings_of(&marks_on_open(&s, CLOSE_TABLE_PATH)),
        on_disk - 1
    );
    // saved, the disk is the file again (the test's disk is the old text, so the finding is back)
    s.all_within(QUIET);
    s.save(CLOSE_TABLE_PATH);
    assert_eq!(findings_of(&s.marks(CLOSE_TABLE_PATH)), on_disk);
    s.stop();
}

#[test]
fn a_fix_the_editor_refuses_leaves_the_disks_marks() {
    let dir = bistro_with_the_fix();
    let s = served_editing(dir.path());
    let on_disk = findings_of(&marks_on_open(&s, CLOSE_TABLE_PATH));
    let plan = plan_use_port(&s, json!(4));
    let id = apply_asked(&s, &plan);
    let asked = edit_asked(&s);
    assert_eq!(
        findings_of(&marks_on_open(&s, CLOSE_TABLE_PATH)),
        on_disk - 1
    );
    s.reply(asked.id, json!({ "applied": false }));
    s.response(id);
    assert_eq!(findings_of(&s.marks(CLOSE_TABLE_PATH)), on_disk);
    assert_eq!(findings_of(&marks_on_open(&s, CLOSE_TABLE_PATH)), on_disk);
    s.stop();
}

// ---- M2c-2: owned regions in the editor ----

const COMMANDS_MOD: &str = "src/booking/commands/mod.rs";
const CLOSE_TABLE_RS: &str = "src/booking/commands/close_table.rs";
const SYNCED_NOTICE: &str =
    "spellbook: synced region modules in src/booking/commands/mod.rs (+1 \u{2212}0)";

/// A command file written by hand: the region `modules` of `commands/mod.rs` should list it.
fn write_close_table(root: &Path) {
    write_file(
        root,
        CLOSE_TABLE_RS,
        "//! Close a table.\n\npub fn close_table() {}\n",
    );
}

/// The messages the server shows, in order, read until it goes quiet.
fn said_within(s: &Served, within: Duration) -> Vec<String> {
    s.all_within(within)
        .into_iter()
        .filter_map(|m| match m {
            Message::Notification(n) if n.method == "window/showMessage" => {
                n.params["message"].as_str().map(String::from)
            }
            _ => None,
        })
        .collect()
}

/// The next message the server shows.
fn next_said(s: &Served) -> String {
    match s.wait_for(
        WAIT,
        |m| matches!(m, Message::Notification(n) if n.method == "window/showMessage"),
    ) {
        Some(Message::Notification(n)) => n.params["message"].as_str().unwrap().to_string(),
        _ => panic!("nothing said"),
    }
}

fn region_codes(marks: &[Diagnostic]) -> Vec<String> {
    marks
        .iter()
        .map(code_of)
        .filter(|code| code.starts_with("region-"))
        .collect()
}

fn on_disk(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap()
}

#[test]
fn a_save_syncs_a_region_file_not_open_on_disk_and_says_so() {
    let dir = bistro_rs_with_regions();
    let s = served_editing(dir.path());
    write_close_table(dir.path());
    s.open(CLOSE_TABLE_RS);
    s.save(CLOSE_TABLE_RS);
    assert_eq!(next_said(&s), SYNCED_NOTICE);
    assert!(
        on_disk(dir.path(), COMMANDS_MOD)
            .contains("pub mod close_table;\npub mod create_booking;\n"),
        "{}",
        on_disk(dir.path(), COMMANDS_MOD)
    );
    // a second save has nothing to sync, and says nothing
    s.save(CLOSE_TABLE_RS);
    assert!(said_within(&s, QUIET).is_empty());
    s.stop();
}

#[test]
fn a_save_lands_the_sync_of_an_open_region_file_as_an_edit_the_editor_can_undo() {
    let dir = bistro_rs_with_regions();
    let s = served_editing(dir.path());
    s.open(COMMANDS_MOD);
    assert!(region_codes(&s.marks(COMMANDS_MOD)).is_empty());
    let before = on_disk(dir.path(), COMMANDS_MOD);
    write_close_table(dir.path());
    s.save(CLOSE_TABLE_RS);
    let asked = edit_asked(&s);
    assert_eq!(asked.params["label"], "spellbook: sync region modules");
    let documents = asked.params["edit"]["documentChanges"].as_array().unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0]["textDocument"],
        json!({ "uri": s.uri(COMMANDS_MOD), "version": null })
    );
    let landed = with_edits_made(&before, documents[0]["edits"].as_array().unwrap());
    assert!(
        landed.contains("pub mod close_table;\npub mod create_booking;\n"),
        "{landed}"
    );
    s.reply(asked.id, json!({ "applied": true }));
    assert_eq!(next_said(&s), SYNCED_NOTICE);
    // the buffer is the person's to save: the disk is not touched
    assert_eq!(on_disk(dir.path(), COMMANDS_MOD), before);
    // a save of another file before mod.rs is saved sends no second edit
    s.save(CLOSE_TABLE_RS);
    assert!(
        s.wait_for(
            QUIET,
            |m| matches!(m, Message::Request(r) if r.method == "workspace/applyEdit")
        )
        .is_none()
    );
    s.stop();
}

#[test]
fn a_sync_edit_the_editor_refuses_is_said_and_left_for_the_finding() {
    let dir = bistro_rs_with_regions();
    let s = served_editing(dir.path());
    s.open(COMMANDS_MOD);
    s.marks(COMMANDS_MOD);
    write_close_table(dir.path());
    s.save(CLOSE_TABLE_RS);
    let asked = edit_asked(&s);
    s.reply(
        asked.id,
        json!({ "applied": false, "failureReason": "the buffer moved on" }),
    );
    assert_eq!(
        next_said(&s),
        "spellbook: region modules in src/booking/commands/mod.rs was not synced (the editor refused the edit); run spellbook sync"
    );
    let codes = loop {
        let codes = region_codes(&s.marks(COMMANDS_MOD));
        if !codes.is_empty() {
            break codes;
        }
    };
    assert_eq!(codes, ["region-stale"]);
    s.stop();
}

#[test]
fn sync_on_save_off_shows_the_finding_and_says_so_once() {
    let dir = bistro_rs_with_regions();
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook]\nsync_on_save = false\n",
    );
    let s = served_editing(dir.path());
    s.open(COMMANDS_MOD);
    s.marks(COMMANDS_MOD);
    let before = on_disk(dir.path(), COMMANDS_MOD);
    write_close_table(dir.path());
    s.save(CLOSE_TABLE_RS);
    let messages = s.all_within(Duration::from_secs(2));
    let said: Vec<&str> = messages
        .iter()
        .filter_map(|m| match m {
            Message::Notification(n) if n.method == "window/showMessage" => {
                n.params["message"].as_str()
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        said,
        ["spellbook: 1 region out of date; run spellbook sync"]
    );
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::Request(r) if r.method == "workspace/applyEdit"))
    );
    let mod_uri = s.uri(COMMANDS_MOD);
    let republished = messages
        .iter()
        .rev()
        .find(|m| is_publish_for(m, &mod_uri))
        .expect("mod.rs republished");
    let Message::Notification(note) = republished else {
        unreachable!()
    };
    let marks: Vec<Diagnostic> =
        serde_json::from_value(note.params["diagnostics"].clone()).unwrap();
    assert_eq!(region_codes(&marks), ["region-stale"]);
    let stale = marks.iter().find(|m| code_of(m) == "region-stale").unwrap();
    assert_eq!(stale.range.start.line, 2);
    assert_eq!(
        stale.message,
        "region modules is out of date (+1 \u{2212}0)"
    );
    assert_eq!(on_disk(dir.path(), COMMANDS_MOD), before);
    // said once for the life of the server
    s.save(CLOSE_TABLE_RS);
    assert!(said_within(&s, QUIET).is_empty());
    s.stop();
}

#[test]
fn a_stale_region_offers_the_sync_as_its_preferred_quick_fix() {
    let dir = bistro_rs_with_regions();
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook]\nsync_on_save = false\n",
    );
    write_close_table(dir.path());
    let s = served(dir.path());
    s.open(COMMANDS_MOD);
    assert_eq!(region_codes(&s.marks(COMMANDS_MOD)), ["region-stale"]);
    let actions = code_actions(&s, COMMANDS_MOD, 2);
    assert_eq!(actions.len(), 1, "{actions:?}");
    let action = &actions[0];
    assert_eq!(action["title"], "spellbook: Sync region modules");
    assert_eq!(action["isPreferred"], true);
    assert_eq!(action["diagnostics"][0]["code"], "region-stale");
    assert_eq!(
        action["command"]["arguments"],
        json!([{
            "generator": "sync",
            "inputs": { "region": "modules", "file": COMMANDS_MOD },
            "missing": [],
            "fix": { "file": COMMANDS_MOD, "line": 3, "rule": "region-stale", "index": 1,
                     "label": "Sync region modules", "version": null },
        }])
    );
    s.stop();
}

#[test]
fn a_region_not_set_up_offers_to_set_it_up() {
    let dir = bistro_rs_project();
    let s = served(dir.path());
    s.open(COMMANDS_MOD);
    assert_eq!(region_codes(&s.marks(COMMANDS_MOD)), ["region-not-set-up"]);
    let actions = code_actions(&s, COMMANDS_MOD, 0);
    assert_eq!(actions[0]["title"], "spellbook: Set up region modules");
    assert_eq!(
        actions[0]["command"]["arguments"][0]["inputs"],
        json!({ "region": "modules", "file": COMMANDS_MOD, "mode": "init" })
    );
    s.stop();
}

#[test]
fn plan_sync_is_the_refresh_with_region_rows_and_hunks_and_its_apply_lands_in_the_editor() {
    let dir = bistro_rs_with_regions();
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook]\nsync_on_save = false\n",
    );
    write_close_table(dir.path());
    let s = served_editing(dir.path());
    // a quick fix sends its finding too; a sync plan does not need it
    let fix = json!({ "file": COMMANDS_MOD, "line": 3, "rule": "region-stale", "index": 1,
                      "version": 4 });
    let plan = s.command(
        "spellbook.plan",
        json!(["sync", { "region": "modules", "file": COMMANDS_MOD }, fix]),
    );
    assert_eq!(plan["generator"], "sync", "{plan}");
    let files = plan["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], COMMANDS_MOD);
    assert_eq!(files[0]["status"], "changed");
    assert_eq!(files[0]["region"], "modules");
    assert_eq!(
        (files[0]["added"].clone(), files[0]["removed"].clone()),
        (json!(1), Value::Null)
    );
    let rows = files[0]["hunks"][0]["lines"].as_array().unwrap();
    assert!(rows.contains(&json!("+pub mod close_table;")), "{rows:?}");
    let id = apply_asked(&s, &plan);
    let asked = edit_asked(&s);
    assert_eq!(asked.params["label"], "spellbook: sync region modules");
    s.reply(asked.id, json!({ "applied": true }));
    let reply = s.response(id).response_result.unwrap();
    assert_eq!(reply["changed"], json!([COMMANDS_MOD]));
    assert_eq!(reply["stops"], json!([]));
    s.stop();
}

#[test]
fn plan_sync_with_mode_init_sets_the_region_up() {
    let dir = bistro_rs_project();
    let s = served(dir.path());
    let plan = s.command(
        "spellbook.plan",
        json!(["sync", { "region": "ports", "mode": "init", "file": "src/booking/ports/mod.rs" }]),
    );
    let rows: Vec<(Value, Value)> = plan["files"]
        .as_array()
        .unwrap_or_else(|| panic!("{plan}"))
        .iter()
        .map(|file| (file["path"].clone(), file["set_up"].clone()))
        .collect();
    assert_eq!(rows, [(json!("src/booking/ports/mod.rs"), json!(2))]);
    s.stop();
}

#[test]
fn plan_sync_with_nothing_to_do_has_no_files() {
    let dir = bistro_rs_with_regions();
    let s = served(dir.path());
    let plan = s.command("spellbook.plan", json!(["sync", {}]));
    assert_eq!(plan["files"], json!([]), "{plan}");
    s.stop();
}

#[test]
fn a_generators_plan_names_the_region_it_refreshes() {
    let dir = bistro_rs_with_regions();
    let s = served(dir.path());
    let plan = s.command(
        "spellbook.plan",
        json!(["command", { "service": "booking", "command": "cancel_table" }]),
    );
    let region_rows: Vec<(Value, Value)> = plan["files"]
        .as_array()
        .unwrap_or_else(|| panic!("{plan}"))
        .iter()
        .filter(|file| file["region"].is_string())
        .map(|file| (file["path"].clone(), file["region"].clone()))
        .collect();
    assert_eq!(region_rows, [(json!(COMMANDS_MOD), json!("modules"))]);
    s.stop();
}
