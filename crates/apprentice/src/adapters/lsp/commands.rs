//! The capabilities, the code actions on a line, and the commands they run.
//!
//! On a line: a region's actions (*Show code*, *Keep this answer*, *Dismiss*, the offer, the
//! four labels), a waiting question's options, a finished task's (*Show diff*, *Undo this
//! task*, *Show journal*) and a running task's *Stop this task*.

use std::path::Path;
use std::sync::Arc;

use lsp_server::ResponseError;
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionProviderCapability, Command,
    ExecuteCommandOptions, ServerCapabilities,
};
use serde_json::{Value, json};
use sorcerer_lsp::path_of;

use crate::domain::region_record::{RegionRecord, ReplyVerdict};
use crate::domain::triage::Kind;

use super::server_state::{Server, lock};

/// The commands the code actions run.
const COMMANDS: [&str; 10] = [
    "apprentice.do",
    "apprentice.stop",
    "apprentice.showDiff",
    "apprentice.undo",
    "apprentice.journal",
    "apprentice.answer",
    "apprentice.showCode",
    "apprentice.keep",
    "apprentice.dismiss",
    "apprentice.label",
];

pub(super) fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: COMMANDS.iter().map(|command| command.to_string()).collect(),
            ..ExecuteCommandOptions::default()
        }),
        ..ServerCapabilities::default()
    }
}

impl Server {
    /// The actions on the request's lines.
    pub(super) fn code_actions(&self, params: &Value) -> Result<Value, ResponseError> {
        let Some(path) = params
            .pointer("/textDocument/uri")
            .and_then(Value::as_str)
            .and_then(|uri| uri.parse().ok())
            .and_then(|uri| path_of(&uri))
        else {
            return Ok(json!([]));
        };
        let first = params
            .pointer("/range/start/line")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
            + 1;
        let last = params
            .pointer("/range/end/line")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
            + 1;
        let mut actions: Vec<CodeActionOrCommand> = Vec::new();
        for (record, start, end) in self.records_found(&path) {
            if start <= last && first <= end {
                actions.extend(self.region_actions(&record));
            }
        }
        actions.extend(self.ask_actions(&path, first, last));
        for line in first..=last {
            for task in self.tasks_marked_at(&path, line) {
                actions.extend(task_actions(&task, &path));
            }
        }
        if let Some(task) = self.running_task() {
            actions.push(action(
                "apprentice: Stop this task",
                "apprentice.stop",
                json!([task]),
            ));
        }
        Ok(serde_json::to_value(actions).unwrap_or(json!([])))
    }

    fn region_actions(&self, record: &RegionRecord) -> Vec<CodeActionOrCommand> {
        let id = json!([record.region.id]);
        let mut actions = Vec::new();
        if record.offers_a_task() && self.running_task().is_none() {
            actions.push(action(&self.offer_title(), "apprentice.do", id.clone()));
        }
        if let Some(reply) = record
            .reply
            .as_ref()
            .filter(|reply| reply.verdict != ReplyVerdict::Failed)
        {
            if !reply.citations.is_empty() {
                actions.push(action(
                    "apprentice: Show code",
                    "apprentice.showCode",
                    id.clone(),
                ));
            }
            if reply.verdict == ReplyVerdict::Answered {
                actions.push(action(
                    "apprentice: Keep this answer",
                    "apprentice.keep",
                    id.clone(),
                ));
            }
        }
        if record.acting_kind().is_some() {
            actions.push(action("apprentice: Dismiss", "apprentice.dismiss", id));
        }
        actions.extend(label_actions(record));
        actions
    }

    fn ask_actions(&self, path: &Path, first: u32, last: u32) -> Vec<CodeActionOrCommand> {
        lock(&self.asks)
            .iter()
            .filter(|pending| {
                pending.editor_path == path && first <= pending.line && pending.line <= last
            })
            .flat_map(|pending| {
                pending.options.iter().map(|option| {
                    action(
                        &format!("apprentice: {option}"),
                        "apprentice.answer",
                        json!([pending.id, option]),
                    )
                })
            })
            .collect()
    }

    /// Runs a command; what the person must hear is said, and an error answers only a malformed
    /// command.
    pub(super) fn execute(self: &Arc<Self>, params: &Value) -> Result<Value, ResponseError> {
        let command = params["command"].as_str().unwrap_or_default();
        let argument = |n: usize| {
            params["arguments"][n]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        let (first, second) = (argument(0), argument(1));
        let server = Arc::clone(self);
        let on_a_thread = move |work: ServerWork| {
            std::thread::spawn(move || work(&server));
        };
        match command {
            "apprentice.do" => on_a_thread(Box::new(move |server| server.do_region(&first))),
            "apprentice.stop" => self.stop_task(&first),
            "apprentice.showDiff" => self.show_diff(&first, Path::new(&second)),
            "apprentice.undo" => self.undo_task(&first),
            "apprentice.journal" => self.show_journal(&first),
            "apprentice.answer" => self.answered(&first, &second),
            "apprentice.showCode" => self.show_code(&first),
            "apprentice.keep" => self.keep_answer(&first),
            "apprentice.dismiss" => on_a_thread(Box::new(move |server| server.dismiss(&first))),
            "apprentice.label" => {
                let kind = Kind::of_word(&second);
                on_a_thread(Box::new(move |server| server.label(&first, kind)));
            }
            other => {
                return Err(ResponseError {
                    code: lsp_server::ErrorCode::InvalidParams as i32,
                    message: format!("unknown command {other}"),
                    data: None,
                });
            }
        }
        Ok(Value::Null)
    }
}

/// Work a command does off the main loop.
type ServerWork = Box<dyn FnOnce(&Arc<Server>) + Send>;

fn task_actions(task: &str, path: &Path) -> Vec<CodeActionOrCommand> {
    let file = path.to_string_lossy().into_owned();
    vec![
        action(
            "apprentice: Show diff",
            "apprentice.showDiff",
            json!([task, file]),
        ),
        action(
            "apprentice: Undo this task",
            "apprentice.undo",
            json!([task]),
        ),
        action(
            "apprentice: Show journal",
            "apprentice.journal",
            json!([task]),
        ),
    ]
}

/// The four labels of a region: what the person says it was.
fn label_actions(record: &RegionRecord) -> Vec<CodeActionOrCommand> {
    let id = &record.region.id;
    [
        ("a question", "question"),
        ("a claim", "claim"),
        ("an instruction", "instruction"),
        ("nothing", "nothing"),
    ]
    .iter()
    .map(|(said, word)| {
        action(
            &format!("apprentice: this was {said}"),
            "apprentice.label",
            json!([id, word]),
        )
    })
    .collect()
}

fn action(title: &str, command: &str, arguments: Value) -> CodeActionOrCommand {
    let arguments = arguments.as_array().cloned();
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        command: Some(Command {
            title: title.to_string(),
            command: command.to_string(),
            arguments,
        }),
        ..CodeAction::default()
    })
}
