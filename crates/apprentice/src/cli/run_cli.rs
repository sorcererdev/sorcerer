//! The program's entry: parse the arguments, find the project, run the command.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::jsonl_task_store::JsonlTaskStore;
use crate::adapters::state_files::StateFiles;
use crate::adapters::wiring::{WireAgent, live_agents, system_clock};
use crate::ports::clock::Clock;

use super::arguments::{Cli, Cmd};
use super::console::print_usage_or_error;
use super::{cost, doctor, hook, log, lsp, mcp, run, trust};

/// What the commands are built with: the agent and the clock. The tests give their own.
pub struct Wiring {
    pub agent: WireAgent,
    pub clock: Arc<dyn Clock>,
}

impl Wiring {
    /// The live agent (`claude`) and the system clock.
    pub fn live() -> Wiring {
        Wiring {
            agent: live_agents(),
            clock: system_clock(),
        }
    }
}

/// The program's entry: runs the command in the current directory, with the live wiring.
pub fn main(args: Vec<OsString>, env: &dyn Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            let _ = writeln!(err, "apprentice: the current directory: {e}");
            return 2;
        }
    };
    main_with(args, &cwd, env, &Wiring::live(), out, err)
}

/// [`main`] with the directory and the wiring given, so tests can run a command with doubles.
#[doc(hidden)]
pub fn main_with(
    args: Vec<OsString>,
    cwd: &Path,
    env: &dyn Env,
    wiring: &Wiring,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let code = match Cli::try_parse_from(args) {
        Ok(cli) => match run_command(cli, cwd, env, wiring, out, err) {
            Ok(code) => code,
            Err(e) => {
                let _ = writeln!(err, "apprentice: {e:#}");
                2
            }
        },
        Err(e) => print_usage_or_error(&e, out, err),
    };
    let _ = out.flush();
    let _ = err.flush();
    code
}

fn run_command(
    cli: Cli,
    cwd: &Path,
    env: &dyn Env,
    wiring: &Wiring,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    // the helpers find their task from the environment, not from here: the other commands open
    // the project holding the current directory
    let project = || Project::open(cwd);
    match cli.cmd {
        Some(Cmd::Lsp) => lsp::lsp(),
        Some(Cmd::Mcp) => mcp::mcp(env, out),
        Some(Cmd::Hook { event }) => hook::hook(&event, env, out),
        None => run::run_here(&cli.run, cwd, project()?, env, wiring, out),
        Some(Cmd::Doctor) => doctor::doctor(&project()?, env, out),
        Some(Cmd::Trust { revoke }) => trust::trust(&project()?, env, revoke, out),
        Some(Cmd::Cost { since, json }) => {
            let ledger = JsonlLedger::for_project(&project()?)?;
            cost::cost(&ledger, since.as_deref(), wiring.clock.offset(), json, out)
        }
        Some(Cmd::Log { n }) => {
            let store = JsonlTaskStore::at(StateFiles::for_project(&project()?)?.tasks());
            let n = n.map_or(log::DEFAULT_COUNT, |n| n as usize);
            log::show_log(&store, n, out, err)
        }
    }
}
