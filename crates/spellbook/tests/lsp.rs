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
use support::{bistro_project, test_cache, use_corpus_rules, vendored_project, write_file};
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
        on_disk
            .lines()
            .nth(handle.range.start.line as usize)
            .unwrap()
            .contains("NotImplementedError"),
        "{on_disk}"
    );
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

// ---- Task C8: findings ----

const CLOSE: &str = "src/bistro/booking/commands/close_table.py";

fn code_of(mark: &Diagnostic) -> String {
    match mark.code.as_ref() {
        Some(lsp_types::NumberOrString::String(code)) => code.clone(),
        other => format!("{other:?}"),
    }
}

#[test]
fn saving_a_command_with_sqlite3_shows_a_warning() {
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
    assert_eq!(
        finding.severity,
        Some(lsp_types::DiagnosticSeverity::WARNING)
    );
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
