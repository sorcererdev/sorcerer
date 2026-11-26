//! A task: one instruction the person accepted, run by one agent session, and its record.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use sorcerer_core::domain::fnv::fnv64;

/// The record's shape; a line of another version is skipped when read.
pub const TASK_RECORD_VERSION: u32 = 1;

/// Where a task is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    /// The agent is working.
    Running,
    /// The agent is waiting for the person to answer a question.
    Asking,
    /// The agent finished its turn.
    Done,
    /// The agent failed (the program could not start, or it said it failed).
    Failed,
    /// The person stopped it.
    Stopped,
    /// The server ended while it ran (found at the next start).
    Interrupted,
    /// The person took its changes back.
    Undone,
    /// The person saved what it changed.
    Kept,
}

impl TaskState {
    /// Whether the task can change nothing more: everything but running and asking.
    pub fn is_over(self) -> bool {
        !matches!(self, TaskState::Running | TaskState::Asking)
    }
}

/// One file a task changed: its path relative to the root, written with `/`, and whether the
/// task created it. Its text before is kept beside the journal (`tasks/<id>/before/<path>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskFile {
    pub path: String,
    pub new: bool,
}

/// One line of `tasks.jsonl`: a task as it stood at one moment. A task is written again at each
/// change of state, so the last line of an id is the task now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub version: u32,
    pub id: String,
    /// What the person asked for, in their words (the region's text).
    pub instruction: String,
    /// Where it was asked: the file (relative, `/`) and the line, from 1.
    pub file: String,
    pub line: u32,
    /// The agent's session, so a later version can resume it.
    pub session_id: Option<String>,
    pub state: TaskState,
    pub files: Vec<TaskFile>,
    /// What the agent reported spending, in dollars (an estimate on a subscription).
    pub cost_usd: f64,
    pub turns: u32,
    pub started: DateTime<Utc>,
    pub ended: Option<DateTime<Utc>>,
    /// A short remark: why it failed, what the agent said last.
    pub note: Option<String>,
}

impl TaskRecord {
    /// A task that has just started.
    pub fn started(id: &str, instruction: &str, file: &str, line: u32, at: DateTime<Utc>) -> Self {
        TaskRecord {
            version: TASK_RECORD_VERSION,
            id: id.to_string(),
            instruction: instruction.to_string(),
            file: file.to_string(),
            line,
            session_id: None,
            state: TaskState::Running,
            files: Vec::new(),
            cost_usd: 0.0,
            turns: 0,
            started: at,
            ended: None,
            note: None,
        }
    }

    /// How long it ran, in seconds; up to `now` while it runs.
    pub fn seconds(&self, now: DateTime<Utc>) -> f64 {
        let end = self.ended.unwrap_or(now);
        (end - self.started).num_milliseconds().max(0) as f64 / 1000.0
    }
}

/// Everything read back from `tasks.jsonl`: the last line of each task, oldest task first, and
/// how many lines did not read.
#[derive(Debug, Default, Clone)]
pub struct TaskRows {
    pub tasks: Vec<TaskRecord>,
    pub skipped: usize,
}

/// The last record of each task, in the order the tasks started.
///
/// Example: lines `a running`, `b running`, `a done` give `a done`, `b running`.
pub fn latest_of_each(records: Vec<TaskRecord>) -> Vec<TaskRecord> {
    records.into_iter().fold(Vec::new(), |mut latest, record| {
        match latest
            .iter_mut()
            .find(|known: &&mut TaskRecord| known.id == record.id)
        {
            Some(known) => *known = record,
            None => latest.push(record),
        }
        latest
    })
}

/// A task's id: `t` and eight hex digits from the time and the instruction, so two tasks never
/// share one by chance.
///
/// Example: `t3f9a01c2`.
pub fn task_id(at: DateTime<Utc>, instruction: &str) -> String {
    let seed = format!(
        "{}\n{instruction}",
        at.timestamp_nanos_opt().unwrap_or_default()
    );
    format!("t{:08x}", fnv64(seed.as_bytes()) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    #[test]
    fn the_last_line_of_a_task_is_the_task_now() {
        let a = TaskRecord::started("a", "do a", "x.txt", 1, at(0));
        let b = TaskRecord::started("b", "do b", "x.txt", 2, at(1));
        let a_done = TaskRecord {
            state: TaskState::Done,
            ..a.clone()
        };
        let latest = latest_of_each(vec![a, b, a_done]);
        assert_eq!(latest.len(), 2);
        assert_eq!(
            (latest[0].id.as_str(), latest[0].state),
            ("a", TaskState::Done)
        );
        assert_eq!(latest[1].id, "b");
    }

    #[test]
    fn ids_differ_by_time_and_instruction() {
        let one = task_id(at(0), "refuse late cancellations");
        assert_eq!(one.len(), 9);
        assert!(one.starts_with('t'));
        assert_ne!(one, task_id(at(1), "refuse late cancellations"));
        assert_ne!(one, task_id(at(0), "something else"));
    }

    #[test]
    fn only_running_and_asking_are_not_over() {
        assert!(!TaskState::Running.is_over());
        assert!(!TaskState::Asking.is_over());
        assert!(TaskState::Done.is_over());
        assert!(TaskState::Interrupted.is_over());
    }
}
