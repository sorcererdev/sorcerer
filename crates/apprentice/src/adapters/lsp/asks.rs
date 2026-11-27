//! The agent's questions waiting in the editor: each one a mark on its line and one code
//! action per option, until the person picks one or the wait ends.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crossbeam_channel::Sender;
use lsp_types::MessageType;
use sorcerer_lsp::Answer;

use crate::domain::mediums::{Ask, AskWay};
use crate::ports::editor::Asked;

use super::server_state::{Server, lock};

/// A question waiting on a line.
#[derive(Clone)]
pub(super) struct PendingAsk {
    pub id: String,
    pub editor_path: PathBuf,
    /// From 1.
    pub line: u32,
    pub question: String,
    pub options: Vec<String>,
    pub answer: Sender<String>,
}

impl Server {
    /// Asks the person, the way the agent chose when the editor can: the picker
    /// (`window/showMessageRequest`) or choices on the line (code actions); waits at most
    /// `wait`. A picker the editor cannot show falls back to the line, and a line it has none for
    /// to the picker.
    pub(super) fn ask_person(&self, task: &str, ask: &Ask, wait: Duration) -> Asked {
        let on_the_line = matches!(ask.how, AskWay::Actions | AskWay::Edit) && ask.at.is_some();
        if on_the_line {
            return self.ask_on_the_line(task, ask, wait);
        }
        match self.ask_in_the_picker(ask, wait) {
            Some(Answer::Picked(picked)) => Asked::Answered(picked),
            Some(Answer::Unsupported) if ask.at.is_some() => self.ask_on_the_line(task, ask, wait),
            _ => Asked::NoAnswer,
        }
    }

    fn ask_in_the_picker(&self, ask: &Ask, wait: Duration) -> Option<Answer> {
        let (sender, answer) = crossbeam_channel::bounded(1);
        let options: Vec<&str> = ask.options.iter().map(String::as_str).collect();
        self.client.ask(&ask.question, &options, move |picked| {
            let _ = sender.send(picked);
        });
        answer.recv_timeout(wait).ok()
    }

    fn ask_on_the_line(&self, task: &str, ask: &Ask, wait: Duration) -> Asked {
        let Some((file, line)) = &ask.at else {
            return Asked::NoAnswer;
        };
        let (sender, answer) = crossbeam_channel::bounded(1);
        let pending = PendingAsk {
            id: format!("{task}-{}", self.asks_put.fetch_add(1, Ordering::SeqCst) + 1),
            editor_path: self.root.join(file),
            line: *line,
            question: ask.question.clone(),
            options: ask.options.clone(),
            answer: sender,
        };
        let (id, path) = (pending.id.clone(), pending.editor_path.clone());
        lock(&self.asks).push(pending);
        self.publish(&path);
        self.client.say(
            MessageType::INFO,
            &format!(
                "apprentice asks: {} (choices on {file}:{line})",
                ask.question
            ),
        );
        let answered = answer.recv_timeout(wait);
        lock(&self.asks).retain(|pending| pending.id != id);
        self.publish(&path);
        answered.map_or(Asked::NoAnswer, Asked::Answered)
    }

    /// The person picked an option of a question on a line.
    pub(super) fn answered(&self, id: &str, option: &str) {
        let pending = lock(&self.asks)
            .iter()
            .find(|pending| pending.id == id)
            .cloned();
        if let Some(pending) = pending {
            let _ = pending.answer.send(option.to_string());
        }
    }
}
