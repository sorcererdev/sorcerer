//! What a product says to the editor, from any thread: diagnostics, a progress line, a message
//! once, a log line, a document to show, a question with answers. Also what the host knows
//! about the editor: which documents are open and what it can show.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crossbeam_channel::Sender;
use lsp_server::{Message, Notification, Request, RequestId, Response, ResponseError};
use lsp_types::{
    Diagnostic, LogMessageParams, MessageActionItem, MessageType, NumberOrString, ProgressParams,
    ProgressParamsValue, PublishDiagnosticsParams, Range, ShowDocumentParams, ShowMessageParams,
    ShowMessageRequestParams, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd, WorkDoneProgressReport,
};
use serde_json::Value;

use crate::uri::uri_of;

/// The way to the editor. Cloning is cheap; every clone speaks over the same connection.
#[derive(Clone)]
pub struct Client {
    shared: Arc<Shared>,
}

/// What the editor said it can do, at initialize.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Abilities {
    /// `window.workDoneProgress`: it shows a progress line the server starts.
    pub work_done_progress: bool,
    /// `textDocument.publishDiagnostics.relatedInformation`: it shows a diagnostic's links.
    pub related_information: bool,
}

struct Shared {
    sender: Sender<Message>,
    abilities: Abilities,
    state: Mutex<State>,
}

/// What changes while the server runs.
#[derive(Default)]
struct State {
    open: BTreeSet<PathBuf>,
    said: HashSet<String>,
    waiting_for_reply: HashMap<RequestId, OnReply>,
    requests_sent: i32,
}

/// What to do with the editor's reply to a request the server sent.
type OnReply = Box<dyn FnOnce(Result<Value, ResponseError>) + Send>;

/// The editor's answer to [`Client::ask`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The person picked this answer.
    Picked(String),
    /// The question was closed without an answer.
    Closed,
    /// The editor cannot show a question with answers.
    Unsupported,
}

impl Client {
    pub(crate) fn new(sender: Sender<Message>, abilities: Abilities) -> Client {
        Client {
            shared: Arc::new(Shared {
                sender,
                abilities,
                state: Mutex::new(State::default()),
            }),
        }
    }

    /// Replaces a file's diagnostics in the editor; an empty list clears them.
    pub fn publish(&self, path: &Path, diagnostics: Vec<Diagnostic>) {
        self.notify(
            "textDocument/publishDiagnostics",
            PublishDiagnosticsParams {
                uri: uri_of(path),
                diagnostics,
                version: None,
            },
        );
    }

    /// Starts a progress line with a title and a first message. Every call on the line does
    /// nothing when the editor did not say it shows server-started progress.
    ///
    /// Example: `work_done("scry", "reviewing 1 unit")`, later `.report("reviewing 2 units")`,
    /// then `.end(None)`.
    pub fn work_done(&self, title: &str, message: &str) -> WorkDone {
        if !self.shared.abilities.work_done_progress {
            return WorkDone { line: None };
        }
        let token = NumberOrString::String(format!("sorcerer-{}", self.next_request_number()));
        let line = Arc::new(Mutex::new(ProgressLine {
            client: self.clone(),
            token: token.clone(),
            created: Created::Waiting(Vec::new()),
        }));
        let on_created = Arc::clone(&line);
        self.request(
            "window/workDoneProgress/create",
            WorkDoneProgressCreateParams { token },
            Box::new(move |reply| lock(&on_created).created_with(reply.is_ok())),
        );
        let work_done = WorkDone { line: Some(line) };
        work_done.send(WorkDoneProgress::Begin(WorkDoneProgressBegin {
            title: title.to_string(),
            message: Some(message.to_string()),
            ..WorkDoneProgressBegin::default()
        }));
        work_done
    }

    /// Shows a message, at most once per key for the life of the server. Answers whether it
    /// was shown now.
    pub fn say_once(&self, key: &str, kind: MessageType, text: &str) -> bool {
        if !lock(&self.shared.state).said.insert(key.to_string()) {
            return false;
        }
        self.notify(
            "window/showMessage",
            ShowMessageParams {
                typ: kind,
                message: text.to_string(),
            },
        );
        true
    }

    /// A line in the editor's log for this server, never shown unasked.
    pub fn log(&self, text: &str) {
        self.notify(
            "window/logMessage",
            LogMessageParams {
                typ: MessageType::LOG,
                message: text.to_string(),
            },
        );
    }

    /// Opens a file in the editor and moves the cursor there, with a range selected when one
    /// is given.
    pub fn show_document(&self, path: &Path, selection: Option<Range>) {
        self.request(
            "window/showDocument",
            ShowDocumentParams {
                uri: uri_of(path),
                external: Some(false),
                take_focus: Some(true),
                selection,
            },
            Box::new(|_| {}),
        );
    }

    /// Asks a question with answers to pick from. `on_answer` runs on the main loop when the
    /// reply arrives, and never if none does; nothing waits for it.
    pub fn ask(
        &self,
        text: &str,
        answers: &[&str],
        on_answer: impl FnOnce(Answer) + Send + 'static,
    ) {
        let actions = answers
            .iter()
            .map(|title| MessageActionItem {
                title: (*title).to_string(),
                properties: HashMap::new(),
            })
            .collect();
        self.request(
            "window/showMessageRequest",
            ShowMessageRequestParams {
                typ: MessageType::INFO,
                message: text.to_string(),
                actions: Some(actions),
            },
            Box::new(move |reply| on_answer(answer_from(reply))),
        );
    }

    /// Whether the editor has this file open.
    pub fn is_open(&self, path: &Path) -> bool {
        lock(&self.shared.state).open.contains(path)
    }

    /// Every file the editor has open, in path order.
    pub fn open_paths(&self) -> Vec<PathBuf> {
        lock(&self.shared.state).open.iter().cloned().collect()
    }

    /// Whether the editor shows a diagnostic's related locations.
    pub fn related_information(&self) -> bool {
        self.shared.abilities.related_information
    }

    pub(crate) fn opened(&self, path: &Path) {
        lock(&self.shared.state).open.insert(path.to_path_buf());
    }

    pub(crate) fn closed(&self, path: &Path) {
        lock(&self.shared.state).open.remove(path);
    }

    /// Hands the editor's reply to whatever waits for it; a reply nothing waits for is
    /// dropped.
    pub(crate) fn replied(&self, response: Response) {
        let waiting = lock(&self.shared.state)
            .waiting_for_reply
            .remove(&response.id);
        if let Some(on_reply) = waiting {
            on_reply(response.response_result);
        }
    }

    fn notify(&self, method: &str, params: impl serde::Serialize) {
        let note = Notification::new(method.to_string(), params);
        let _ = self.shared.sender.send(Message::Notification(note));
    }

    fn request(&self, method: &str, params: impl serde::Serialize, on_reply: OnReply) {
        let id = RequestId::from(self.next_request_number());
        lock(&self.shared.state)
            .waiting_for_reply
            .insert(id.clone(), on_reply);
        let request = Request::new(id, method.to_string(), params);
        let _ = self.shared.sender.send(Message::Request(request));
    }

    fn next_request_number(&self) -> i32 {
        let mut state = lock(&self.shared.state);
        state.requests_sent += 1;
        state.requests_sent
    }
}

/// The answer in a reply to `window/showMessageRequest`: a null result is a closed question,
/// an error an editor that cannot ask.
fn answer_from(reply: Result<Value, ResponseError>) -> Answer {
    match reply {
        Err(_) => Answer::Unsupported,
        Ok(Value::Null) => Answer::Closed,
        Ok(picked) => serde_json::from_value::<MessageActionItem>(picked)
            .map_or(Answer::Closed, |item| Answer::Picked(item.title)),
    }
}

/// A progress line in the editor, from [`Client::work_done`].
pub struct WorkDone {
    line: Option<Arc<Mutex<ProgressLine>>>,
}

/// A progress line's token and whether the editor has made it yet: the protocol lets the
/// server use the token only after the editor answered the create request, so what is sent
/// before then waits.
struct ProgressLine {
    client: Client,
    token: NumberOrString,
    created: Created,
}

enum Created {
    Waiting(Vec<WorkDoneProgress>),
    Yes,
    Refused,
}

impl WorkDone {
    /// Changes the line's message.
    pub fn report(&self, message: &str) {
        self.send(WorkDoneProgress::Report(WorkDoneProgressReport {
            message: Some(message.to_string()),
            ..WorkDoneProgressReport::default()
        }));
    }

    /// Ends the line, with a last message when one is given.
    pub fn end(self, message: Option<&str>) {
        self.send(WorkDoneProgress::End(WorkDoneProgressEnd {
            message: message.map(str::to_string),
        }));
    }

    fn send(&self, progress: WorkDoneProgress) {
        if let Some(line) = &self.line {
            lock(line).send(progress);
        }
    }
}

impl ProgressLine {
    fn send(&mut self, progress: WorkDoneProgress) {
        match &mut self.created {
            Created::Waiting(held) => held.push(progress),
            Created::Yes => self.client.notify(
                "$/progress",
                ProgressParams {
                    token: self.token.clone(),
                    value: ProgressParamsValue::WorkDone(progress),
                },
            ),
            Created::Refused => {}
        }
    }

    /// The editor answered the create request: what was held is sent, or dropped if it
    /// refused.
    fn created_with(&mut self, made: bool) {
        let held = match std::mem::replace(&mut self.created, Created::Refused) {
            Created::Waiting(held) => held,
            _ => Vec::new(),
        };
        if made {
            self.created = Created::Yes;
            held.into_iter().for_each(|progress| self.send(progress));
        }
    }
}

/// The lock, also after a thread panicked while holding it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
