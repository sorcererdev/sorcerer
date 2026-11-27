//! The agent seam: what runs a task. The live one is Claude Code headless (`claude -p` with
//! stream-json in and out); a second adapter (another harness) can come without touching the
//! use cases.

use std::path::PathBuf;
use std::time::Duration;

use sorcerer_core::Result;

use crate::domain::agent_event::AgentEvent;

/// How the agent's helper processes reach the server that runs the task: its loopback port
/// and token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WayBack {
    pub port: u16,
    pub token: String,
}

/// One task for the agent to run.
#[derive(Debug, Clone)]
pub struct TaskRequest {
    /// The task's id, which its helper processes name in every request.
    pub task: String,
    /// The first message: the task (`task_prompt`).
    pub prompt: String,
    /// What is appended to the agent's own system prompt: the editor's rules.
    pub system: String,
    /// The model alias (`sonnet`).
    pub model: String,
    /// The most the task may spend, in dollars.
    pub budget_usd: f64,
    pub max_turns: u32,
    /// The project root, where the agent runs.
    pub root: PathBuf,
    pub way_back: WayBack,
}

/// What the next look at a running agent found.
#[derive(Debug, Clone, PartialEq)]
pub enum NextEvent {
    Event(AgentEvent),
    /// Nothing within the wait; it still runs.
    Waiting,
    /// It ended (its output closed).
    Ended,
}

/// Starts tasks.
pub trait Agent: Send + Sync {
    /// Starts the agent on the task; it begins at once with the request's prompt.
    fn start(&self, request: &TaskRequest) -> Result<Box<dyn Running>>;
}

/// A running agent.
pub trait Running: Send {
    /// The next thing it did, waiting at most `wait`.
    fn next_event(&mut self, wait: Duration) -> NextEvent;
    /// Says there is nothing more to send: it ends once its turn is over.
    fn close_input(&mut self);
    /// Stops it and everything it started.
    fn stop(&mut self);
}
