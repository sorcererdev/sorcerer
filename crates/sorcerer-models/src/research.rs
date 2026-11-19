//! The researcher: a model that may look things up in the project (read a
//! file, grep, glob) under fences and answer with citations. Two ways to
//! get one: `claude -p` brings its own tools ([`claude_code`]); any other
//! provider gets core's own loop ([`tool_loop`]). Nothing a researcher
//! writes enters a file.

pub mod claude_code;
pub mod recorded;
pub mod tool_loop;
pub mod tools;

use std::path::PathBuf;
use std::time::Duration;

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;

use crate::Result;

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

/// Every implementation appends its errand to the ledger before
/// returning, success or failure (the recorded double excepted: it spends
/// nothing).
pub trait Researcher: Send + Sync {
    fn research(&self, r: Research, cancel: &Cancel) -> Result<Researched>;
}

/// A ledger row for R with only a cost (the recorded double's).
pub(crate) fn errand_for(r: &Research, cost: Option<f64>) -> Errand {
    Errand {
        at: chrono::Utc::now(),
        product: r.product.clone(),
        verb: r.verb.clone(),
        model: r.model.clone(),
        provider: "recorded".into(),
        cost_usd: cost,
        input_tokens: 0,
        output_tokens: 0,
        seconds: 0.0,
        ok: true,
        note: None,
    }
}
