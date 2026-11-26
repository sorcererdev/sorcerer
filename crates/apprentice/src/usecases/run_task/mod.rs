//! Running one task: start it, follow it, answer its helper processes, finish it.
//!
//! - `start.rs`: `start_task` (the day's cap, the record, the first message, the agent).
//! - `follow.rs`: `follow_task` (the status line, the final text, the end).
//! - `answer_request.rs`: `answer_request`, which routes a helper's request to
//!   `tool_calls.rs` (`answer_tool_call`, the six mediums), `permissions.rs`
//!   (`answer_permission`, Bash asked once) or `feed.rs` (`edits_for_agent`, the hook).
//! - `finish.rs`: `finish_task` (the record, the ledger, the done message).
//! - `task_deps.rs`: `TaskDeps` and `LiveTask`, what a task's threads share.

mod answer_request;
mod feed;
mod finish;
mod follow;
mod permissions;
mod start;
mod task_deps;
mod tool_calls;

pub use answer_request::answer_request;
pub use feed::STOP_EVENT;
pub use finish::{Ending, TASK_VERB, finish_task};
pub use follow::follow_task;
pub use start::{StartedTask, TaskOrder, TaskRefused, start_task};
pub use task_deps::{LiveTask, TaskDeps};
