//! What a product says to the editor, from any thread: diagnostics, a progress line, a message
//! once, a log line, a document to show, a question with answers, an edit to apply. Also what the host knows
//! about the editor: which documents are open, their text when the product tracks it, and what
//! the editor can show.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crossbeam_channel::Sender;
use lsp_server::{Message, Notification, Request, RequestId, Response, ResponseError};
use lsp_types::{
    ApplyWorkspaceEditParams, ApplyWorkspaceEditResponse, Diagnostic, LogMessageParams,
    MessageActionItem, MessageType, NumberOrString, ProgressParams, ProgressParamsValue,
    PublishDiagnosticsParams, Range, ShowDocumentParams, ShowMessageParams,
    ShowMessageRequestParams, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd, WorkDoneProgressReport, WorkspaceEdit,
};
use serde_json::Value;

use crate::serve::TracksText;
use crate::uri::uri_of;

/// The way to the editor. Cloning is cheap; every clone speaks over the same connection.
#[derive(Clone)]
pub struct Client {
    shared: Arc<Shared>,
}

/// What the editor said it can do, at initialize.
#[derive(Debug, Clone, Default)]
pub(crate) struct Abilities {
    /// `window.workDoneProgress`: it shows a progress line the server starts.
    pub work_done_progress: bool,
    /// `textDocument.publishDiagnostics.relatedInformation`: it shows a diagnostic's links.
    pub related_information: bool,
    /// `workspace.applyEdit`: it applies an edit the server sends.
    pub apply_edit: bool,
    /// `workspace.workspaceEdit.documentChanges`: it takes edits tied to a document's version.
    /// Also true when it announces any `resourceOperations`, which exist only inside
    /// `documentChanges`.
    pub document_changes: bool,
    /// `workspace.workspaceEdit.resourceOperations` holds `create`: an edit may create a file.
    pub create_files: bool,
    /// `workspace.workspaceEdit.resourceOperations` holds `delete`: an edit may delete a file.
    pub delete_files: bool,
    /// `initializationOptions.drawsWorking`: it draws the units being worked on, from
    /// `sorcerer/working`. Only Sorcerer's own editor extensions say so.
    pub draws_working: bool,
    /// `initializationOptions.handles`: the `sorcerer/…` notifications it handles beyond
    /// `sorcerer/working`, such as `sorcerer/ghost`. Only Sorcerer's own editor extensions say so.
    pub handles: Vec<String>,
}

struct Shared {
    sender: Sender<Message>,
    abilities: Abilities,
    tracks_text: TracksText,
    state: Mutex<State>,
}

/// What changes while the server runs.
#[derive(Default)]
struct State {
    open: BTreeSet<PathBuf>,
    /// Each open document's version and text, for a product that tracks text.
    texts: HashMap<PathBuf, (i32, String)>,
    said: HashSet<String>,
    /// Whether the editor was last told some units are being worked on.
    working_shown: bool,
    waiting_for_reply: HashMap<RequestId, OnReply>,
    requests_sent: i32,
}

/// What to do with the editor's reply to a request the server sent.
type OnReply = Box<dyn FnOnce(Result<Value, ResponseError>) + Send>;

/// The editor's answer to [`Client::apply_edit`]: whether it applied the edit, and when it did
/// not, the reason it gave (if any).
///
/// Example: an editor whose document moved on since the edit was made answers
/// `Applied { applied: false, failure_reason: Some("version moved".into()) }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub applied: bool,
    pub failure_reason: Option<String>,
}

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
    pub(crate) fn new(
        sender: Sender<Message>,
        abilities: Abilities,
        tracks_text: TracksText,
    ) -> Client {
        Client {
            shared: Arc::new(Shared {
                sender,
                abilities,
                tracks_text,
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

    /// Tells the editor which units are being worked on, so it can write `text` after the
    /// first line of each (`⟳ scry: reviewing…`). Replaces what was shown before. Each place is
    /// a file and the unit's first line, from 1. Sent as the notification `sorcerer/working`,
    /// and only to an editor that said at initialize it draws it (`drawsWorking` in the
    /// initialization options); to any other this does nothing.
    ///
    /// Example: `show_working("⟳ scry: reviewing…", &[(orders_py, 12)])`, and when the work is
    /// over, `clear_working()`.
    pub fn show_working(&self, text: &str, places: &[(PathBuf, u32)]) {
        if !self.shared.abilities.draws_working {
            return;
        }
        let units: Vec<Value> = places
            .iter()
            .map(|(path, line)| {
                serde_json::json!({
                    "uri": uri_of(path),
                    "line": line.saturating_sub(1),
                    "text": text,
                })
            })
            .collect();
        lock(&self.shared.state).working_shown = !units.is_empty();
        self.notify("sorcerer/working", serde_json::json!({ "units": units }));
    }

    /// Ends what [`Client::show_working`] showed. Says nothing when nothing is shown.
    pub fn clear_working(&self) {
        let was_shown = std::mem::take(&mut lock(&self.shared.state).working_shown);
        if was_shown {
            self.show_working("", &[]);
        }
    }

    /// Shows a message, every time it is called: for what the person just asked for, such as
    /// a command refused because another is running.
    pub fn say(&self, kind: MessageType, text: &str) {
        self.notify(
            "window/showMessage",
            ShowMessageParams {
                typ: kind,
                message: text.to_string(),
            },
        );
    }

    /// Shows a message, at most once per key for the life of the server. Answers whether it
    /// was shown now.
    pub fn say_once(&self, key: &str, kind: MessageType, text: &str) -> bool {
        if !lock(&self.shared.state).said.insert(key.to_string()) {
            return false;
        }
        self.say(kind, text);
        true
    }

    /// Sends a notification of sorcerer's own (`sorcerer/ghost`, `sorcerer/note`,
    /// `sorcerer/showDiff`) when the editor said at initialize that it handles it
    /// (`initializationOptions.handles`), and answers whether it was sent; to any other editor it
    /// sends nothing, so the product falls back to what every editor shows.
    ///
    /// Example: `notify_sorcerer("sorcerer/note", json!({ "uri": … }))` is `false` for an editor
    /// that did not list `sorcerer/note`, and the product shows the note with `show_document`.
    pub fn notify_sorcerer(&self, method: &str, params: Value) -> bool {
        debug_assert!(method.starts_with("sorcerer/"), "{method}");
        if !self.handles(method) {
            return false;
        }
        self.notify(method, params);
        true
    }

    /// Whether the editor said it handles the `sorcerer/…` notification.
    pub fn handles(&self, method: &str) -> bool {
        self.shared
            .abilities
            .handles
            .iter()
            .any(|handled| handled == method)
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

    /// Asks the editor to apply an edit, under a label it may show (in its undo history, say).
    /// `on_reply` runs on the main loop when the reply arrives, and never if none does; nothing
    /// waits for it. An error reply (an editor that cannot apply edits) is not applied, with
    /// the error's message as the reason.
    pub fn apply_edit(
        &self,
        label: &str,
        edit: WorkspaceEdit,
        on_reply: impl FnOnce(Applied) + Send + 'static,
    ) {
        self.request(
            "workspace/applyEdit",
            ApplyWorkspaceEditParams {
                label: Some(label.to_string()),
                edit,
            },
            Box::new(move |reply| on_reply(applied_from(reply))),
        );
    }

    /// Whether the editor applies edits the server sends, tied to documents' versions
    /// (`workspace/applyEdit` with `documentChanges`). Without both, a product cannot land an
    /// edit safely.
    pub fn can_apply_edits(&self) -> bool {
        self.shared.abilities.apply_edit && self.shared.abilities.document_changes
    }

    /// Whether an edit the server sends may create a file.
    pub fn can_create_files(&self) -> bool {
        self.shared.abilities.create_files
    }

    /// Whether an edit the server sends may delete a file.
    pub fn can_delete_files(&self) -> bool {
        self.shared.abilities.delete_files
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

    /// The version and text of an open document, as the editor last sent them. `None` when
    /// the document is not open, or the product does not track text.
    ///
    /// Example: after an open at version 1 with `a` and a change to version 2 with `b`,
    /// `text_of(path)` is `Some((2, "b"))`.
    pub fn text_of(&self, path: &Path) -> Option<(i32, String)> {
        lock(&self.shared.state).texts.get(path).cloned()
    }

    pub(crate) fn tracks_text(&self) -> bool {
        self.shared.tracks_text == TracksText::Yes
    }

    /// The editor opened a file, with its version and text when the notification held them.
    pub(crate) fn opened(&self, path: &Path, document: Option<(i32, String)>) {
        let mut state = lock(&self.shared.state);
        state.open.insert(path.to_path_buf());
        if let (true, Some(document)) = (self.tracks_text(), document) {
            state.texts.insert(path.to_path_buf(), document);
        }
    }

    pub(crate) fn changed(&self, path: &Path, version: i32, text: &str) {
        lock(&self.shared.state)
            .texts
            .insert(path.to_path_buf(), (version, text.to_string()));
    }

    pub(crate) fn closed(&self, path: &Path) {
        let mut state = lock(&self.shared.state);
        state.open.remove(path);
        state.texts.remove(path);
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

    /// Drops every callback still waiting for a reply, unrun. Called when the connection
    /// ends: a callback may hold a [`Client`], and a client keeps the connection open.
    pub(crate) fn forget_waiting(&self) {
        let forgotten = std::mem::take(&mut lock(&self.shared.state).waiting_for_reply);
        drop(forgotten);
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

/// The result in a reply to `workspace/applyEdit`: an error, or a result that is not an
/// answer, is not applied.
fn applied_from(reply: Result<Value, ResponseError>) -> Applied {
    match reply {
        Err(error) => Applied {
            applied: false,
            failure_reason: Some(error.message),
        },
        Ok(result) => match serde_json::from_value::<ApplyWorkspaceEditResponse>(result) {
            Ok(answer) => Applied {
                applied: answer.applied,
                failure_reason: answer.failure_reason,
            },
            Err(error) => Applied {
                applied: false,
                failure_reason: Some(format!("an answer that is not one: {error}")),
            },
        },
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
