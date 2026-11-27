//! Apprentice: a coding agent whose whole interface is the editor. The person is the sorcerer;
//! the apprentice works in their workshop, speaks only through the editor (a message, a line in
//! the margin, grey text, a change in the buffer, a note beside the code, a question), is fed
//! the person's edits after every tool call, and everything it makes is reviewed with the
//! editor's own tools.
//!
//! `usecases/` is the service layer (`run_task`, `report_cost`, `show_log`); `domain/` data and
//! rules with no I/O; `ports/` the traits a use case talks through (`Agent`, `Editor`, the
//! stores); `adapters/` implement them (`claude -p` with stream-json, the loopback port, the
//! MCP server and hooks the agent runs, the files under `.sorcerer/state/apprentice/`); `cli/`
//! parses, calls one use case, prints.

pub mod adapters;
pub mod cli;
pub mod domain;
pub mod ports;
pub mod usecases;
