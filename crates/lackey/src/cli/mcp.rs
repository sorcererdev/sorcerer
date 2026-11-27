//! `lackey mcp`: the medium tools as an MCP server over standard input and output. The
//! agent starts it with the way back in its environment.

use std::io::Write;

use anyhow::{Result, anyhow};
use sorcerer_core::ports::env::Env;

use crate::adapters::helper_way::HelperWay;
use crate::adapters::mcp_server::serve_mcp;

/// Serves until the agent closes the input.
pub(super) fn mcp(env: &dyn Env, out: &mut dyn Write) -> Result<i32> {
    let way = HelperWay::from_env(env).ok_or_else(|| anyhow!(started_by_hand_text()))?;
    serve_mcp(std::io::stdin().lock(), out, &way)?;
    Ok(0)
}

fn started_by_hand_text() -> String {
    "lackey: this is started by a task's agent, which sets LACKEY_PORT, LACKEY_TOKEN and LACKEY_TASK".to_string()
}
