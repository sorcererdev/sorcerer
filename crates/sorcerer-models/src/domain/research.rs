//! The researcher: a model that may look things up in the project (read a
//! file, grep, glob) under fences and answer with citations. Two ways to
//! get one: `claude -p` brings its own tools ([`claude_code`]); any other
//! provider gets core's own loop ([`tool_loop`]). Nothing a researcher
//! writes enters a file.

use std::path::PathBuf;
use std::time::Duration;

use sorcerer_core::domain::errand::Errand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Read,
    Grep,
    Glob,
}

impl Tool {
    /// The name `claude` gives the tool.
    pub fn claude_name(&self) -> &'static str {
        match self {
            Tool::Read => "Read",
            Tool::Grep => "Grep",
            Tool::Glob => "Glob",
        }
    }
}

/// One research errand. The fences are the researcher's own, built from
/// the project, so they cannot be forgotten here.
#[derive(Debug, Clone)]
pub struct Research {
    pub product: String,
    pub verb: String,
    pub prompt: String,
    /// An alias the provider resolves (`sonnet`).
    pub model: String,
    pub tools: Vec<Tool>,
    /// The most this errand may spend; a timeout is charged this.
    pub cap_usd: f64,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct Researched {
    pub answer: String,
    pub errand: Errand,
    /// The project files it read (when the researcher can tell).
    pub reads: Vec<PathBuf>,
}
