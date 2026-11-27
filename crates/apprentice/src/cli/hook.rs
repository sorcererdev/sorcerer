//! `apprentice hook post-tool-use|stop`: a hook of the agent's. It reads the hook's JSON on its
//! input (and needs nothing from it: the task is in the environment), asks the server for the
//! person's edits, and prints what the agent should be told. It always exits 0, so a hook never
//! stops the agent.

use std::io::{Read, Write};

use anyhow::Result;
use sorcerer_core::ports::env::Env;

use crate::adapters::helper_way::HelperWay;
use crate::adapters::hooks::hook_output;

pub(super) fn hook(event: &str, env: &dyn Env, out: &mut dyn Write) -> Result<i32> {
    if let Some(way) = HelperWay::from_env(env) {
        // the hook's JSON is read away (the task is in the environment); only an agent's hook
        // has an input that ends
        let mut ignored = String::new();
        let _ = std::io::stdin().read_to_string(&mut ignored);
        let output = hook_output(event, &way);
        if !output.is_empty() {
            writeln!(out, "{output}")?;
        }
    }
    Ok(0)
}
