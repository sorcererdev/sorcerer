//! scry's language server, driven over an in-memory connection with the pass doubles.
#![allow(clippy::too_many_lines)]
mod support;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};
use scry::adapters::lsp::{STALE_LINE, ServerConfig, serve};
use scry::adapters::wiring::Wiring;
use scry::domain::finding::{Dismissal, Finding};
use scry::ports::claim_checker::ClaimChecker;
use scry::usecases::review_change::ReviewDeps;
use serde_json::{Value, json};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::language::GrammarSource;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_lsp::uri_of;
use support::{Doubles, FixedClock, StubReviewer, commit, git_project, write};
use tempfile::TempDir;

const ORDERS: &str = "def cancel(order):\n    return order.cancel()\n\n\ndef place(order):\n    return order.place()\n";

/// How long a test waits for something the server must send.
const WAIT: Duration = Duration::from_secs(10);
/// How long a test watches for something the server must not send.
const QUIET: Duration = Duration::from_millis(300);

// ---- the harness ----

/// A pass's wiring in the server's tests: the shared doubles with the pass's two cancels.
struct TestWiring {
    doubles: Arc<Doubles>,
    review: Cancel,
    rest: Cancel,
}

impl Wiring for TestWiring {
    fn deps(&self) -> ReviewDeps<'_> {
        ReviewDeps {
            cancel: &self.rest,
            review_cancel: &self.review,
            ..self.doubles.deps()
        }
    }
}

type Wire = Box<dyn Fn(Arc<Doubles>, Cancel, Cancel) -> anyhow::Result<TestWiring> + Send + Sync>;

/// How a test's server is set up: what the editor says it can do, and how a pass's wiring
/// is built (the doubles, unless a test says otherwise).
struct Setup {
    capabilities: Value,
    wire: Wire,
    trust_dir: Option<PathBuf>,
    /// The claim checker, when not the doubles' (which says right).
    checker: Option<Box<dyn ClaimChecker + Send>>,
}

impl Default for Setup {
    fn default() -> Setup {
        Setup {
            capabilities: json!({}),
            wire: Box::new(|doubles, review, rest| {
                Ok(TestWiring {
                    doubles,
                    review,
                    rest,
                })
            }),
            trust_dir: None,
            checker: None,
        }
    }
}

/// The project at a root with its grammars on the vendored sources.
fn open_with_vendored_grammars(root: &Path) -> anyhow::Result<Project> {
    let mut project = Project::open(root)?;
    for name in ["python", "rust"] {
        project
            .languages
            .override_grammar(name, GrammarSource::Local(support::vendored(name)));
    }
    Ok(project)
}

/// The editor's end of a server running on a thread over DIR, with the doubles.
struct Served {
    editor: Connection,
    doubles: Arc<Doubles>,
    server: Option<JoinHandle<()>>,
    /// Messages received and not yet taken by a test.
    inbox: RefCell<VecDeque<Message>>,
    requests_sent: std::cell::Cell<i32>,
    _trust_home: TempDir,
}

/// The server over DIR with the default setup.
fn served(dir: &Path, reviewer: StubReviewer) -> Served {
    served_with(dir, reviewer, Setup::default())
}

fn served_with(dir: &Path, reviewer: StubReviewer, setup: Setup) -> Served {
    let mut doubles = Doubles::new(dir, reviewer);
    if let Some(checker) = setup.checker {
        doubles.checker = checker;
    }
    let doubles = Arc::new(doubles);
    let trust_home = TempDir::new().unwrap();
    let trust = TrustList::at(setup.trust_dir.as_deref().unwrap_or(trust_home.path()));
    let for_wire = Arc::clone(&doubles);
    let wire = setup.wire;
    let cfg = ServerConfig {
        env: Arc::new(MapEnv::default()),
        open: Box::new(open_with_vendored_grammars),
        cache: support::test_cache(),
        trust: Some(trust),
        settle: Duration::from_millis(20),
        clock: Arc::new(FixedClock::at("2026-11-20T12:00:00Z".parse().unwrap())),
        wire: Box::new(move |_project, review, rest| wire(Arc::clone(&for_wire), review, rest)),
    };
    let (server_end, editor_end) = Connection::memory();
    let server = std::thread::spawn(move || serve(server_end, cfg).unwrap());
    let s = Served {
        editor: editor_end,
        doubles,
        server: Some(server),
        inbox: RefCell::new(VecDeque::new()),
        requests_sent: std::cell::Cell::new(0),
        _trust_home: trust_home,
    };
    s.initialize(dir, setup.capabilities);
    s
}

impl Served {
    fn initialize(&self, root: &Path, capabilities: Value) {
        let id = self.request(
            "initialize",
            json!({ "capabilities": capabilities, "rootUri": uri_of(root).as_str() }),
        );
        self.response(id);
        self.notify("initialized", json!({}));
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

    fn reply(&self, id: RequestId, result: Result<Value, lsp_server::ResponseError>) {
        let response = Response {
            id,
            response_result: result,
        };
        self.editor
            .sender
            .send(Message::Response(response))
            .unwrap();
    }

    fn open(&self, path: &Path) {
        self.document("textDocument/didOpen", path);
    }

    fn save(&self, path: &Path) {
        self.document("textDocument/didSave", path);
    }

    fn close(&self, path: &Path) {
        self.document("textDocument/didClose", path);
    }

    fn document(&self, method: &str, path: &Path) {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        self.notify(
            method,
            json!({ "textDocument": {
                "uri": uri_of(path).as_str(), "languageId": "", "version": 1, "text": text
            }}),
        );
    }

    /// The next message from the server before the deadline. Requests a real editor answers
    /// by itself (a progress token, a document to show) are answered here too, and still
    /// handed to the test.
    fn pull(&self, deadline: Instant) -> Option<Message> {
        let left = deadline.saturating_duration_since(Instant::now());
        let message = self.editor.receiver.recv_timeout(left).ok()?;
        if let Message::Request(r) = &message {
            match r.method.as_str() {
                "window/workDoneProgress/create" => self.reply(r.id.clone(), Ok(Value::Null)),
                "window/showDocument" => self.reply(r.id.clone(), Ok(json!({ "success": true }))),
                _ => {}
            }
        }
        Some(message)
    }

    /// The first message, received before or within WITHIN, that passes the test; the others
    /// stay for later.
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

    /// Every message received before or within WITHIN.
    fn all_within(&self, within: Duration) -> Vec<Message> {
        let deadline = Instant::now() + within;
        while let Some(message) = self.pull(deadline) {
            self.inbox.borrow_mut().push_back(message);
        }
        self.inbox.borrow_mut().drain(..).collect()
    }

    /// The next diagnostics published for PATH within WITHIN.
    fn next_diagnostics(&self, path: &Path, within: Duration) -> Option<Vec<Diagnostic>> {
        let uri = uri_of(path).as_str().to_string();
        let message = self.wait_for(within, |m| is_publish_for(m, &uri))?;
        let Message::Notification(note) = message else {
            unreachable!()
        };
        Some(serde_json::from_value(note.params["diagnostics"].clone()).unwrap())
    }

    /// The response to request ID.
    fn response(&self, id: i32) -> Response {
        let id = RequestId::from(id);
        match self.wait_for(WAIT, |m| matches!(m, Message::Response(r) if r.id == id)) {
            Some(Message::Response(r)) => r,
            _ => panic!("no response to request {id}"),
        }
    }

    /// Shuts the server down and waits for it to end.
    fn stop(mut self) {
        let id = self.request("shutdown", Value::Null);
        self.response(id);
        self.notify("exit", Value::Null);
        self.server.take().unwrap().join().unwrap();
    }
}

fn is_publish_for(m: &Message, uri: &str) -> bool {
    matches!(m, Message::Notification(n)
        if n.method == "textDocument/publishDiagnostics" && n.params["uri"] == uri)
}

fn notifications<'m>(messages: &'m [Message], method: &str) -> Vec<&'m Value> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::Notification(n) if n.method == method => Some(&n.params),
            _ => None,
        })
        .collect()
}

// ---- the project ----

/// A git project with orders.py committed.
fn project() -> TempDir {
    let dir = git_project();
    write(dir.path(), "orders.py", ORDERS);
    commit(dir.path(), "orders");
    dir
}

fn orders(dir: &TempDir) -> PathBuf {
    dir.path().join("orders.py")
}

/// The fingerprint `cancel` has in orders.py now.
fn cancel_print(dir: &Path) -> String {
    let (_, view) = support::project_with_vendored_grammars(dir);
    let units = view.units(Path::new("orders.py")).unwrap();
    let cancel = units.iter().find(|u| u.name == "cancel").unwrap();
    cancel.fingerprint.as_str().to_string()
}

/// An open finding of ISSUE_TYPE in `cancel`, on line 2.
fn in_cancel(dir: &Path, id: &str, issue_type: &str) -> Finding {
    let print = cancel_print(dir);
    support::open_finding(id, issue_type, "orders.py", Some(("cancel", 1, &print)), 2)
}

fn seed(dir: &Path, findings: Vec<Finding>) {
    let project = Project::open(dir).unwrap();
    support::put_findings(&project, findings);
}

fn code(d: &Diagnostic) -> &str {
    match &d.code {
        Some(NumberOrString::String(code)) => code,
        _ => panic!("no code: {d:?}"),
    }
}

// ---- findings as diagnostics ----

#[test]
fn opening_a_file_publishes_its_open_findings() {
    let dir = project();
    seed(
        dir.path(),
        vec![support::open_finding("b00001", "bug", "orders.py", None, 3)],
    );
    let s = served(dir.path(), StubReviewer::new());
    s.open(&orders(&dir));
    let shown = s.next_diagnostics(&orders(&dir), WAIT).unwrap();
    assert_eq!(shown.len(), 1, "{shown:?}");
    let d = &shown[0];
    assert_eq!(d.range.start.line, 2);
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(d.source.as_deref(), Some("scry"));
    assert_eq!(code(d), "bug");
    assert_eq!(d.message, "statement of b00001");
    assert_eq!(d.data, Some(json!("b00001")));
    s.stop();
}

#[test]
fn every_other_issue_type_is_a_warning() {
    let dir = project();
    let mut convention = in_cancel(dir.path(), "c00001", "convention");
    convention.rule = Some("R1".into());
    seed(dir.path(), vec![convention]);
    let s = served(dir.path(), StubReviewer::new());
    s.open(&orders(&dir));
    let shown = s.next_diagnostics(&orders(&dir), WAIT).unwrap();
    assert_eq!(shown[0].severity, Some(DiagnosticSeverity::WARNING));
    assert!(
        shown[0].message.ends_with("\nrule: R1"),
        "{}",
        shown[0].message
    );
    assert_eq!(
        shown[0].range.end.character,
        "    return order.cancel()".len() as u32
    );
    s.stop();
}

#[test]
fn a_stale_finding_says_so() {
    let dir = project();
    seed(dir.path(), vec![in_cancel(dir.path(), "b00001", "bug")]);
    let s = served(dir.path(), StubReviewer::new());
    s.open(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT).unwrap().len(), 1);
    // two lines above it, and its own code changed
    write(
        dir.path(),
        "orders.py",
        &format!(
            "import os\n\n{}",
            ORDERS.replace("order.cancel()", "order.cancel(now=True)")
        ),
    );
    s.save(&orders(&dir));
    let shown = s.next_diagnostics(&orders(&dir), WAIT).unwrap();
    assert_eq!(code(&shown[0]), "bug-stale");
    assert_eq!(shown[0].message.lines().last(), Some(STALE_LINE));
    assert_eq!(shown[0].range.start.line, 3);
    s.stop();
}

#[test]
fn evidence_is_related_information_when_the_client_takes_it() {
    let dir = project();
    seed(dir.path(), vec![in_cancel(dir.path(), "b00001", "bug")]);
    let setup = Setup {
        capabilities: json!({ "textDocument": { "publishDiagnostics": { "relatedInformation": true } } }),
        ..Setup::default()
    };
    let s = served_with(dir.path(), StubReviewer::new(), setup);
    s.open(&orders(&dir));
    let shown = s.next_diagnostics(&orders(&dir), WAIT).unwrap();
    let related = shown[0].related_information.as_ref().unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].message, "evidence");
    assert_eq!(related[0].location.range.start.line, 1);
    assert!(
        related[0].location.uri.as_str().ends_with("/orders.py"),
        "{:?}",
        related[0].location.uri
    );
    s.stop();
}

#[test]
fn evidence_is_absent_when_it_does_not() {
    let dir = project();
    seed(dir.path(), vec![in_cancel(dir.path(), "b00001", "bug")]);
    let s = served(dir.path(), StubReviewer::new());
    s.open(&orders(&dir));
    let shown = s.next_diagnostics(&orders(&dir), WAIT).unwrap();
    assert!(shown[0].related_information.is_none());
    s.stop();
}

#[test]
fn at_most_three_a_file() {
    let dir = project();
    let findings = (0..4)
        .map(|i| in_cancel(dir.path(), &format!("b0000{i}"), "bug"))
        .collect();
    seed(dir.path(), findings);
    let s = served(dir.path(), StubReviewer::new());
    s.open(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT).unwrap().len(), 3);
    s.stop();
}

#[test]
fn closing_publishes_an_empty_list() {
    let dir = project();
    seed(dir.path(), vec![in_cancel(dir.path(), "b00001", "bug")]);
    let s = served(dir.path(), StubReviewer::new());
    s.open(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT).unwrap().len(), 1);
    s.close(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT), Some(Vec::new()));
    s.stop();
}

#[test]
fn a_file_that_is_not_open_gets_nothing() {
    let dir = project();
    seed(dir.path(), vec![in_cancel(dir.path(), "b00001", "bug")]);
    let s = served(dir.path(), StubReviewer::new());
    s.save(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), QUIET), None);
    assert!(s.doubles.reviewer.asks().is_empty());
    s.stop();
}

#[test]
fn a_file_outside_the_project_or_with_no_language_is_ignored() {
    let dir = project();
    write(dir.path(), "notes.txt", "a note\n");
    // a nested project with its own .sorcerer/ directory
    let nested = dir.path().join("vendor/lib");
    std::fs::create_dir_all(nested.join(".sorcerer")).unwrap();
    write(&nested, "pay.py", "def charge(x):\n    return x\n");
    let elsewhere = project();
    let s = served(dir.path(), StubReviewer::new());
    for path in [
        dir.path().join("notes.txt"),
        nested.join("pay.py"),
        orders(&elsewhere),
    ] {
        s.open(&path);
        s.save(&path);
        assert_eq!(s.next_diagnostics(&path, QUIET), None, "{}", path.display());
    }
    s.stop();
}

#[test]
fn a_dismissal_in_the_terminal_shows_at_the_next_save() {
    let dir = project();
    seed(dir.path(), vec![in_cancel(dir.path(), "b00001", "bug")]);
    let s = served(dir.path(), StubReviewer::new());
    s.open(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT).unwrap().len(), 1);
    let project = Project::open(dir.path()).unwrap();
    support::update_store(&project, |findings| {
        findings
            .dismiss(
                "b00001",
                Dismissal::Here,
                "2026-11-20T12:00:00Z".parse().unwrap(),
            )
            .unwrap();
    })
    .unwrap();
    s.save(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT), Some(Vec::new()));
    s.stop();
}

#[test]
fn scry_off_clears() {
    let dir = project();
    seed(dir.path(), vec![in_cancel(dir.path(), "b00001", "bug")]);
    let s = served(dir.path(), StubReviewer::new());
    s.open(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT).unwrap().len(), 1);
    write(
        dir.path(),
        ".sorcerer/config.toml",
        "[scry]\nenabled = false\n",
    );
    s.save(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT), Some(Vec::new()));
    s.stop();
}

/// After SPOIL, two saves log each error once and publish nothing, and a request is still answered;
/// after REPAIR, a save publishes the finding again.
fn a_broken_file_is_logged_and_the_next_save_recovers(
    spoil: impl Fn(&Path),
    repair: impl Fn(&Path),
) {
    let dir = project();
    seed(dir.path(), vec![in_cancel(dir.path(), "b00001", "bug")]);
    let s = served(dir.path(), StubReviewer::new());
    s.open(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT).unwrap().len(), 1);
    spoil(dir.path());
    s.save(&orders(&dir));
    s.save(&orders(&dir));
    let id = s.request("scry/nothing", Value::Null);
    assert_eq!(s.response(id).response_result.unwrap_err().code, -32601);
    let said = s.all_within(QUIET);
    let logged = notifications(&said, "window/logMessage");
    let distinct: std::collections::BTreeSet<String> =
        logged.iter().map(|l| l["message"].to_string()).collect();
    assert!(!logged.is_empty(), "nothing logged");
    assert_eq!(distinct.len(), logged.len(), "logged twice: {said:?}");
    assert!(notifications(&said, "textDocument/publishDiagnostics").is_empty());
    repair(dir.path());
    s.save(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT).unwrap().len(), 1);
    s.stop();
}

#[test]
fn a_broken_config_is_logged_and_the_next_save_recovers() {
    a_broken_file_is_logged_and_the_next_save_recovers(
        |dir| write(dir, ".sorcerer/config.toml", "[scry"),
        |dir| std::fs::remove_file(dir.join(".sorcerer/config.toml")).unwrap(),
    );
}

#[test]
fn a_store_from_a_newer_scry_is_logged_and_the_next_save_recovers() {
    let store = |dir: &Path| dir.join(".sorcerer/state/scry/findings.json");
    let kept = std::sync::Mutex::new(String::new());
    a_broken_file_is_logged_and_the_next_save_recovers(
        |dir| {
            *kept.lock().unwrap() = std::fs::read_to_string(store(dir)).unwrap();
            std::fs::write(store(dir), "{\"version\": 99, \"data\": {}}").unwrap();
        },
        |dir| std::fs::write(store(dir), &*kept.lock().unwrap()).unwrap(),
    );
}

// ---- from a save to a pass ----

/// A git project with orders.py committed, then `cancel` changed.
fn changed_project() -> TempDir {
    let dir = project();
    change_cancel(dir.path(), "order.cancel(now=True)");
    dir
}

/// Rewrites `cancel`'s body to return CALL.
fn change_cancel(dir: &Path, call: &str) {
    write(dir, "orders.py", &ORDERS.replace("order.cancel()", call));
}

/// A review reply holding these findings on orders.py's line 2: (issue type, statement).
fn reply_with(findings: &[(&str, &str)]) -> String {
    let findings: Vec<Value> = findings
        .iter()
        .map(|(issue_type, statement)| {
            json!({
                "issue_type": issue_type,
                "file": "orders.py",
                "lines": [2, 2],
                "statement": statement,
                "evidence": [{"file": "orders.py", "lines": [2, 2]}],
            })
        })
        .collect();
    json!({ "findings": findings, "open": [] }).to_string()
}

fn bug_reviewer() -> StubReviewer {
    StubReviewer::new().reply(
        &["orders.py"],
        &reply_with(&[("bug", "cancel ignores now")]),
    )
}

/// The editor announces it shows progress.
fn with_progress() -> Setup {
    Setup {
        capabilities: json!({ "window": { "workDoneProgress": true } }),
        ..Setup::default()
    }
}

impl Served {
    /// The diagnostics published for PATH, skipping publishes until one passes the test.
    fn diagnostics_until(
        &self,
        path: &Path,
        within: Duration,
        wanted: impl Fn(&[Diagnostic]) -> bool,
    ) -> Option<Vec<Diagnostic>> {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let shown = self.next_diagnostics(path, left)?;
            if wanted(&shown) {
                return Some(shown);
            }
        }
    }

    /// The value of the next `$/progress` of this kind (`begin`, `report`, `end`).
    fn progress(&self, kind: &str, within: Duration) -> Option<Value> {
        let message = self.wait_for(within, |m| {
            matches!(m, Message::Notification(n)
                if n.method == "$/progress" && n.params["value"]["kind"] == kind)
        })?;
        let Message::Notification(note) = message else {
            unreachable!()
        };
        Some(note.params["value"].clone())
    }

    fn asks(&self) -> usize {
        self.doubles.reviewer.asks().len()
    }

    /// Waits until the reviewer has been asked COUNT times.
    fn wait_for_asks(&self, count: usize) {
        let deadline = Instant::now() + WAIT;
        while self.asks() < count {
            assert!(
                Instant::now() < deadline,
                "{} asks, not {count}",
                self.asks()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// A gate a stub blocks on until the test opens it.
#[derive(Clone, Default)]
struct Gate(Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);

impl Gate {
    fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }

    /// Waits until the gate opens, at most WAIT; whether it opened.
    fn wait(&self) -> bool {
        let opened = self.0.0.lock().unwrap();
        let (opened, _) = self.0.1.wait_timeout_while(opened, WAIT, |o| !*o).unwrap();
        *opened
    }
}

#[test]
fn a_save_reviews_after_the_settle_and_publishes() {
    let dir = changed_project();
    let s = served(dir.path(), bug_reviewer());
    s.open(&orders(&dir));
    s.save(&orders(&dir));
    let shown = s.diagnostics_until(&orders(&dir), Duration::from_secs(2), |d| !d.is_empty());
    let shown = shown.expect("a diagnostic within 2 s");
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].message, "cancel ignores now");
    assert_eq!(s.asks(), 1);
    s.stop();
}

#[test]
fn two_quick_saves_are_one_pass() {
    let dir = changed_project();
    let s = served_with(dir.path(), bug_reviewer(), with_progress());
    s.open(&orders(&dir));
    s.save(&orders(&dir));
    s.save(&orders(&dir));
    assert!(s.progress("end", WAIT).is_some());
    let said = s.all_within(QUIET);
    let begins = notifications(&said, "$/progress")
        .iter()
        .filter(|p| p["value"]["kind"] == "begin")
        .count();
    assert_eq!(begins, 1, "{said:?}");
    assert_eq!(s.asks(), 1);
    s.stop();
}

#[test]
fn a_clean_save_is_silent() {
    let dir = project();
    let s = served_with(dir.path(), bug_reviewer(), with_progress());
    s.open(&orders(&dir));
    assert_eq!(s.next_diagnostics(&orders(&dir), WAIT), Some(Vec::new()));
    s.save(&orders(&dir));
    let said = s.all_within(QUIET);
    assert!(notifications(&said, "$/progress").is_empty(), "{said:?}");
    assert!(
        notifications(&said, "window/showMessage").is_empty(),
        "{said:?}"
    );
    let published = notifications(&said, "textDocument/publishDiagnostics");
    assert_eq!(published.len(), 1, "{said:?}");
    assert_eq!(published[0]["diagnostics"], json!([]));
    assert!(
        !said.iter().any(|m| matches!(m, Message::Request(_))),
        "{said:?}"
    );
    assert_eq!(s.asks(), 0);
    s.stop();
}

#[test]
fn progress_begins_with_the_units_and_ends() {
    let dir = changed_project();
    let s = served_with(dir.path(), bug_reviewer(), with_progress());
    s.open(&orders(&dir));
    s.save(&orders(&dir));
    let create = s.wait_for(
        WAIT,
        |m| matches!(m, Message::Request(r) if r.method == "window/workDoneProgress/create"),
    );
    assert!(create.is_some());
    let begin = s.progress("begin", WAIT).unwrap();
    assert_eq!(begin["title"], "scry");
    assert_eq!(begin["message"], "reviewing 1 unit");
    let end = s.progress("end", WAIT).unwrap();
    assert!(end.get("message").is_none(), "{end}");
    s.stop();
}

#[test]
fn a_save_during_a_pass_waits_for_it() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = project();
    write(dir.path(), "pay.py", "def charge(x):\n    return x\n");
    commit(dir.path(), "pay");
    change_cancel(dir.path(), "order.cancel(now=True)");
    let gate = Gate::default();
    let in_flight = Arc::new(AtomicUsize::new(0));
    let most_in_flight = Arc::new(AtomicUsize::new(0));
    let reviewer = {
        let (gate, in_flight, most) = (gate.clone(), in_flight.clone(), most_in_flight.clone());
        StubReviewer::new().on_review(move |ask| {
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            if ask.units[0].file == Path::new("orders.py") {
                assert!(gate.wait(), "the test never opened the gate");
            }
            in_flight.fetch_sub(1, Ordering::SeqCst);
        })
    };
    let s = served(dir.path(), reviewer);
    let pay = dir.path().join("pay.py");
    s.save(&orders(&dir));
    s.wait_for_asks(1);
    write(dir.path(), "pay.py", "def charge(x):\n    return x * 2\n");
    s.save(&pay);
    std::thread::sleep(QUIET);
    assert_eq!(s.asks(), 1, "the second save waits for the pass");
    gate.open();
    s.wait_for_asks(2);
    let asks = s.doubles.reviewer.asks();
    assert_eq!(asks[1].units[0].file, Path::new("pay.py"));
    assert_eq!(most_in_flight.load(Ordering::SeqCst), 1);
    s.stop();
}

#[test]
fn an_overtaking_save_cancels_the_review_call() {
    let dir = changed_project();
    let in_call = Gate::default();
    let saw_cancel = Arc::new(AtomicBool::new(false));
    let reviewer = {
        let (in_call, saw_cancel) = (in_call.clone(), saw_cancel.clone());
        StubReviewer::new().on_review_cancellable(move |ask, cancel| {
            if !ask.units[0].text.contains("now=True") {
                return;
            }
            in_call.open();
            let deadline = Instant::now() + WAIT;
            while !cancel.is_cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            saw_cancel.store(cancel.is_cancelled(), Ordering::SeqCst);
        })
    };
    let s = served_with(dir.path(), reviewer, with_progress());
    s.save(&orders(&dir));
    assert!(in_call.wait());
    change_cancel(dir.path(), "order.cancel(later=True)");
    s.save(&orders(&dir));
    let end = s.progress("end", WAIT).unwrap();
    assert!(saw_cancel.load(Ordering::SeqCst));
    assert!(end.get("message").is_none(), "{end}");
    s.wait_for_asks(2);
    let asks = s.doubles.reviewer.asks();
    assert!(
        asks[1].units[0].text.contains("later=True"),
        "{:?}",
        asks[1].units
    );
    s.stop();
}

#[test]
fn a_save_that_leaves_a_sent_unit_alone_does_not_cancel() {
    let dir = changed_project();
    let in_call = Gate::default();
    let saw_cancel = Arc::new(AtomicBool::new(false));
    let reviewer = {
        let (in_call, saw_cancel) = (in_call.clone(), saw_cancel.clone());
        StubReviewer::new().on_review_cancellable(move |_, cancel| {
            in_call.open();
            std::thread::sleep(Duration::from_secs(1));
            saw_cancel.store(cancel.is_cancelled(), Ordering::SeqCst);
        })
    };
    let s = served(dir.path(), reviewer);
    s.save(&orders(&dir));
    assert!(in_call.wait());
    // only `place` changes; `cancel` is still what was sent
    write(
        dir.path(),
        "orders.py",
        &ORDERS
            .replace("order.cancel()", "order.cancel(now=True)")
            .replace("order.place()", "order.place(now=True)"),
    );
    s.save(&orders(&dir));
    s.wait_for_asks(2);
    assert!(!saw_cancel.load(Ordering::SeqCst));
    s.stop();
}

#[test]
fn a_failed_review_ends_the_line_with_the_log_hint() {
    let dir = changed_project();
    let s = served_with(
        dir.path(),
        StubReviewer::new().timing_out(),
        with_progress(),
    );
    s.save(&orders(&dir));
    let end = s.progress("end", WAIT).unwrap();
    assert_eq!(end["message"], "review failed, see \"scry log\"");
    let said = s.all_within(QUIET);
    assert!(
        notifications(&said, "window/showMessage").is_empty(),
        "{said:?}"
    );
    s.stop();
}

#[test]
fn the_day_message_is_said_once_a_day() {
    let dir = changed_project();
    write(
        dir.path(),
        ".sorcerer/config.toml",
        "[scry]\nbudget_per_day = 0.0\n",
    );
    let s = served(dir.path(), bug_reviewer());
    s.save(&orders(&dir));
    let said = s.wait_for(
        WAIT,
        |m| matches!(m, Message::Notification(n) if n.method == "window/showMessage"),
    );
    let Some(Message::Notification(said)) = said else {
        panic!("no message")
    };
    assert_eq!(
        said.params["message"],
        "scry: today's budget ($0.00) is spent. Reviews resume tomorrow; \"scry cost\" shows the days."
    );
    change_cancel(dir.path(), "order.cancel(later=True)");
    s.save(&orders(&dir));
    let more = s.all_within(Duration::from_millis(500));
    assert!(
        notifications(&more, "window/showMessage").is_empty(),
        "{more:?}"
    );
    assert_eq!(s.asks(), 0);
    s.stop();
}

/// A wiring that never builds: as with no Jev key.
fn no_key() -> Setup {
    Setup {
        wire: Box::new(|_, _, _| Err(anyhow::anyhow!("no JEV_API_KEY"))),
        ..Setup::default()
    }
}

#[test]
fn cannot_run_is_said_once_and_only_with_something_to_review() {
    let dir = project();
    let s = served_with(dir.path(), StubReviewer::new(), no_key());
    s.save(&orders(&dir));
    let clean = s.all_within(Duration::from_millis(500));
    assert!(
        notifications(&clean, "window/showMessage").is_empty(),
        "{clean:?}"
    );
    assert!(
        !notifications(&clean, "window/logMessage").is_empty(),
        "{clean:?}"
    );
    change_cancel(dir.path(), "order.cancel(now=True)");
    s.save(&orders(&dir));
    let said = s.wait_for(
        WAIT,
        |m| matches!(m, Message::Notification(n) if n.method == "window/showMessage"),
    );
    let Some(Message::Notification(said)) = said else {
        panic!("no message")
    };
    assert_eq!(
        said.params["message"],
        "scry is not reviewing: no JEV_API_KEY. Run \"scry doctor\"."
    );
    s.save(&orders(&dir));
    let more = s.all_within(Duration::from_millis(500));
    assert!(
        notifications(&more, "window/showMessage").is_empty(),
        "{more:?}"
    );
    s.stop();
}

#[test]
fn a_pass_that_dies_does_not_block_the_next() {
    let dir = changed_project();
    let died = Arc::new(AtomicBool::new(false));
    let setup = Setup {
        wire: {
            let died = died.clone();
            Box::new(move |doubles, review, rest| {
                if !died.swap(true, Ordering::SeqCst) {
                    panic!("the wiring fell over");
                }
                Ok(TestWiring {
                    doubles,
                    review,
                    rest,
                })
            })
        },
        ..Setup::default()
    };
    let s = served_with(dir.path(), bug_reviewer(), setup);
    s.save(&orders(&dir));
    let logged = s.wait_for(
        WAIT,
        |m| matches!(m, Message::Notification(n) if n.method == "window/logMessage"),
    );
    assert!(logged.is_some());
    s.save(&orders(&dir));
    s.wait_for_asks(1);
    s.stop();
}

#[test]
fn a_finding_needing_no_check_shows_before_the_checks_end() {
    use sorcerer_models::domain::claim::{Checked, Claim, Escalation, Verdict};
    let dir = changed_project();
    let checks_may_end = Gate::default();
    let setup = Setup {
        checker: Some({
            let gate = checks_may_end.clone();
            Box::new(move |_: &Claim| {
                gate.wait();
                Ok(Checked {
                    verdict: Verdict::Right,
                    why: "it does".into(),
                    citations: Vec::new(),
                    cited: Vec::new(),
                    escalation: Escalation::No,
                    errands: Vec::new(),
                })
            })
        }),
        ..Setup::default()
    };
    let reviewer = StubReviewer::new().reply(
        &["orders.py"],
        &reply_with(&[
            ("convention", "cancel breaks the naming rule"),
            ("bug", "cancel ignores now"),
        ]),
    );
    let s = served_with(dir.path(), reviewer, setup);
    s.open(&orders(&dir));
    s.save(&orders(&dir));
    let early = s.diagnostics_until(&orders(&dir), WAIT, |d| !d.is_empty());
    let early = early.expect("a finding before the checks end");
    assert_eq!(early.len(), 1, "{early:?}");
    assert_eq!(early[0].message, "cancel breaks the naming rule");
    checks_may_end.open();
    let all = s.diagnostics_until(&orders(&dir), WAIT, |d| d.len() == 2);
    assert!(all.is_some());
    s.stop();
}

#[test]
fn shutdown_answers_while_a_pass_runs() {
    let dir = changed_project();
    let in_call = Gate::default();
    let reviewer = {
        let in_call = in_call.clone();
        StubReviewer::new().on_review_cancellable(move |_, cancel| {
            in_call.open();
            let deadline = Instant::now() + WAIT;
            while !cancel.is_cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        })
    };
    let s = served(dir.path(), reviewer);
    s.save(&orders(&dir));
    assert!(in_call.wait());
    let asked = Instant::now();
    let id = s.request("shutdown", Value::Null);
    s.response(id);
    assert!(asked.elapsed() < Duration::from_secs(2));
    s.notify("exit", Value::Null);
    let mut s = s;
    s.server.take().unwrap().join().unwrap();
}
