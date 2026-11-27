//! The record of a change: one line of `.sorcerer/state/conjure/changes.jsonl` per change, at
//! its final choice. Editor use is what is recorded; the command line records only the ledger.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::facts::Facts;
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::paths::forward_slashes;

use crate::domain::retry_drops::{CheckedDrop, NameKind};

/// The version written on each line.
pub const RECORD_VERSION: u32 = 1;

/// What made the change: the cursor, or a scry finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    Cursor,
    /// The id of the finding.
    Finding(String),
}

/// Which attempt wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Attempt {
    First,
    TryHarder,
}

/// What became of a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Choice {
    /// The person kept it: saved a changed file, or conjured the next change.
    Kept,
    Undone,
    /// Try harder undid it and wrote another.
    Replaced,
    /// The editor refused the edit; nothing landed.
    Dropped,
    NoChange,
    Failed,
    /// The server shut down with the change still marked.
    Unanswered,
    /// The editor said it applied the edit, but some files did not read as the change made
    /// them: the change is not conjure's to undo.
    LandedInPart,
}

/// What the code said about the unit before the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFacts {
    pub parse_errors: Vec<u32>,
    pub todo_lines: Vec<u32>,
    pub body_empty: bool,
}

impl From<&Facts> for RecordedFacts {
    fn from(facts: &Facts) -> RecordedFacts {
        RecordedFacts {
            parse_errors: facts.parse_errors.clone(),
            todo_lines: facts.todo_lines.clone(),
            body_empty: facts.body_empty,
        }
    }
}

/// One file of a change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFile {
    pub path: String,
    pub new: bool,
    pub lines_changed: u32,
    pub parse_errors: Vec<u32>,
}

/// One change, as recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub version: u32,
    pub id: String,
    pub at: DateTime<Utc>,
    /// The file the change was asked in, `/`-separated.
    pub file: String,
    pub line: u32,
    pub unit: String,
    pub trigger: Trigger,
    pub attempt: Attempt,
    /// The change this one replaced, when it is a Try harder.
    pub try_harder_of: Option<String>,
    pub facts: RecordedFacts,
    pub model: String,
    pub goal: Option<String>,
    pub files: Vec<RecordedFile>,
    /// Paths the writer named that were not allowed to land.
    pub dropped: Vec<String>,
    pub seconds: f64,
    pub cost_usd: Option<f64>,
    pub choice: Choice,
    /// What a rules retry dropped of the first answer, with Jev's verdict on each; left out
    /// of the line when there is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retry_drops: Vec<RecordedDrop>,
}

/// One drop of a rules retry, as recorded.
///
/// Example: `{ "name": "std::fs::write", "kind": "call", "file": "src/seat.rs", "line": 36,
/// "verdict": "looks unintended", "rules": ["commands-use-ports: Commands talk to ports"] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedDrop {
    pub name: String,
    /// `call` or `definition`.
    pub kind: String,
    pub file: String,
    pub line: u32,
    /// Jev's verdict in words: `looks unintended`, `maybe unintended`, `not checked`, `moved
    /// elsewhere`, `the rule asked for it`.
    pub verdict: String,
    pub rules: Vec<String>,
}

impl From<&CheckedDrop> for RecordedDrop {
    fn from(drop: &CheckedDrop) -> RecordedDrop {
        RecordedDrop {
            name: drop.dropped.name.clone(),
            kind: match drop.dropped.kind {
                NameKind::Call => "call",
                NameKind::Definition => "definition",
            }
            .into(),
            file: drop.dropped.file.clone(),
            line: drop.dropped.line,
            verdict: drop.verdict.label().into(),
            rules: drop.rules.clone(),
        }
    }
}

/// Every record read back, and how many lines could not be read.
#[derive(Debug, Default)]
pub struct ChangeRows {
    pub records: Vec<ChangeRecord>,
    pub skipped: usize,
}

/// A change's id: six hex digits from when it was asked, the file and the line.
///
/// Example: `change_id(at, Path::new("src/a.txt"), 12)` is something like `3fa9c0`.
pub fn change_id(at: DateTime<Utc>, file: &Path, line: u32) -> String {
    let text = format!("{}|{}|{line}", at.to_rfc3339(), forward_slashes(file));
    fnv64_hex(text.as_bytes())[..6].to_string()
}
