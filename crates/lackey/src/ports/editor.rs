//! The editor seam: what a medium needs of the person's editor. The language server implements
//! it over the protocol; the command line over the disk and the terminal; tests record.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::edits_text::FileEdit;
use crate::domain::mediums::{Ask, MarkSeverity};
use crate::domain::status::TaskProgress;
use crate::domain::task::TaskRecord;

/// What became of a `write_file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    /// It landed in the editor as one undoable change. `before` is the file's text before
    /// (none for a new file), so the task can be undone.
    Landed { before: Option<String> },
    /// The file moved on since the agent last looked (the person edited it, or its unsaved text
    /// is not what the agent read): nothing landed; this is its text now.
    MovedOn { now: String },
    /// It could not land; why, in a sentence for the agent.
    Refused(String),
}

/// The person's answer to a question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    Answered(String),
    /// No answer within the wait, or the question was closed.
    NoAnswer,
}

/// What a medium needs of the editor. Every call is made from a task's threads; one that waits
/// on the person (`ask`, `write_file`) blocks only the calling thread.
pub trait Editor: Send + Sync {
    /// A short message, once.
    fn notify(&self, text: &str);
    /// A line in the margin of a file (relative to the root), with hover text. A mark lives
    /// until the task's next mark on that line or the task ends.
    fn mark(&self, task: &str, file: &Path, line: u32, text: &str, severity: MarkSeverity);
    /// Grey lines under a line, not in the buffer.
    fn ghost(&self, file: &Path, line: u32, lines: &[String]);
    /// A file's whole new text, landed in the editor; the task's last look at it becomes this
    /// text.
    fn write_file(&self, task: &str, file: &Path, text: &str) -> Written;
    /// A markdown note opened beside the code; where it was written.
    fn note(&self, task: &str, title: &str, text: &str) -> sorcerer_core::Result<PathBuf>;
    /// Asks the person and waits at most `wait`.
    fn ask(&self, task: &str, ask: &Ask, wait: Duration) -> Asked;
    /// The person's edits in open files since the task last looked; the look moves to now.
    fn edits_since_look(&self, task: &str) -> Vec<FileEdit>;
    /// What the task does now: at the end of its line and in the status line.
    fn progress(&self, task: &str, progress: &TaskProgress);
    /// The task ended: its done message, its marks on the files it changed, the status line
    /// ended.
    fn finished(&self, task: &TaskRecord, done_text: &str);
}
