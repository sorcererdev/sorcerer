//! The researcher: a model that may look things up in the project (read a
//! file, grep, glob) under fences and answer with citations. Two ways to
//! get one: `claude -p` brings its own tools ([`claude_code`]); any other
//! provider gets core's own loop ([`tool_loop`]). Nothing a researcher
//! writes enters a file.

use std::path::PathBuf;
use std::time::Duration;

use sorcerer_core::domain::errand::Errand;

/// A way the researcher may look at the project: read a file, search file
/// contents (grep), or list files matching a pattern (glob).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Read,
    Grep,
    Glob,
}

impl Tool {
    /// The tool's name in `claude -p`'s `--allowedTools` list, such as `Read`.
    pub fn claude_name(&self) -> &'static str {
        match self {
            Tool::Read => "Read",
            Tool::Grep => "Grep",
            Tool::Glob => "Glob",
        }
    }
}

/// One question for a researcher, with the limits it runs under.
///
/// Example: scry asks sonnet whether a claim about `orders.py` holds, with
/// the read, grep and glob tools, at most $0.10 and 180 seconds. The ledger
/// records the call under product `scry`, verb `check`.
///
/// What the researcher may not read (its fences) is not set here: each
/// researcher builds its own from the project, so no caller can forget them.
#[derive(Debug, Clone)]
pub struct Research {
    /// The product the ledger records the call under (`scry`).
    pub product: String,
    /// What the call is for, in the ledger (`review`, `check`).
    pub verb: String,
    pub prompt: String,
    /// An alias the provider resolves (`sonnet`).
    pub model: String,
    pub tools: Vec<Tool>,
    /// The most this call may spend in dollars. A call that times out was
    /// probably paid for, so the ledger charges it this much.
    pub cap_usd: f64,
    pub timeout: Duration,
}

/// A researcher's answer, the ledger row of the call, and what it read.
#[derive(Debug, Clone)]
pub struct Researched {
    pub answer: String,
    pub errand: Errand,
    /// The project files it read, when the researcher can tell; `claude -p`
    /// cannot, so its list is empty.
    pub reads: Vec<PathBuf>,
}
