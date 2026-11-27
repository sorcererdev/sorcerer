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

    /// A question of more than one line (a command with a heredoc, a script) is said whole
    /// first, as a message, and the picker shows its first line: a picker's prompt is one line.
    fn ask_in_the_picker(&self, ask: &Ask, wait: Duration) -> Option<Answer> {
        let (sender, answer) = crossbeam_channel::bounded(1);
        let options: Vec<&str> = ask.options.iter().map(String::as_str).collect();
        let question = Question::of(&ask.question);
        if question.rest.is_some() {
            self.client
                .say(MessageType::INFO, &format!("lackey asks: {}", ask.question));
        }
        self.client
            .ask(&question.picker_text(), &options, move |picked| {
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
            id: format!(
                "{task}-{}",
                self.asks_put.fetch_add(1, Ordering::SeqCst) + 1
            ),
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
                "lackey asks: {} (choices on {file}:{line})",
                Question::of(&ask.question).first_line_marked()
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

/// A question cut for the places that show one line: its first line, and the rest, if any.
///
/// Example: `lackey wants to run: cat > notes.txt <<EOF\nline one\nEOF` is the first line
/// `lackey wants to run: cat > notes.txt <<EOF` and two more lines.
pub(super) struct Question<'a> {
    pub first_line: &'a str,
    pub rest: Option<&'a str>,
}

impl<'a> Question<'a> {
    pub fn of(question: &'a str) -> Question<'a> {
        match question.split_once('\n') {
            Some((first_line, rest)) if !rest.trim().is_empty() => Question {
                first_line,
                rest: Some(rest),
            },
            _ => Question {
                first_line: question.trim_end(),
                rest: None,
            },
        }
    }

    /// The first line, ending in `…` when there is more.
    pub fn first_line_marked(&self) -> String {
        match self.rest {
            Some(_) => format!("{} …", self.first_line),
            None => self.first_line.to_string(),
        }
    }

    /// What the picker shows: the first line, and how many more there are and where.
    fn picker_text(&self) -> String {
        match self.rest {
            Some(rest) => format!(
                "{} … (+{} lines: the whole of it is in the message before this)",
                self.first_line,
                rest.lines().count()
            ),
            None => self.first_line.to_string(),
        }
    }

    /// A mark's message: the first line, which an editor shows beside the line, then the
    /// rest, which it shows when the mark is hovered or opened.
    pub fn mark_text(&self) -> String {
        match self.rest {
            Some(rest) => format!("lackey asks: {} …\n{rest}", self.first_line),
            None => format!("lackey asks: {}", self.first_line),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_of_several_lines_shows_its_first_and_keeps_the_rest_for_the_hover() {
        let question = Question::of("lackey wants to run: cat > n.txt <<EOF\none\nEOF");
        assert_eq!(
            question.first_line,
            "lackey wants to run: cat > n.txt <<EOF"
        );
        assert_eq!(
            question.mark_text(),
            "lackey asks: lackey wants to run: cat > n.txt <<EOF …\none\nEOF"
        );
        assert_eq!(
            question.picker_text(),
            "lackey wants to run: cat > n.txt <<EOF … (+2 lines: the whole of it is in the message before this)"
        );
        let one = Question::of("A clock?\n");
        assert_eq!((one.first_line, one.rest), ("A clock?", None));
        assert_eq!(one.mark_text(), "lackey asks: A clock?");
    }
}
