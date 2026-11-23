//! The program's entry: parse the arguments, find the project, run the command.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use chrono::{FixedOffset, Local};
use clap::Parser;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::jsonl_change_store::JsonlChangeStore;
use crate::adapters::wiring::{Wire, live};

use super::arguments::{Cli, Cmd, RunArgs};
use super::console::print_usage_or_error;
use super::{cost, doctor, log, lsp, run, trust};
use crate::domain::config::conjure_config;
use crate::domain::record::Attempt;

/// The program's entry: runs the command in the current directory, with the real models, files
/// and clock.
pub fn main(args: Vec<OsString>, env: &dyn Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            let _ = writeln!(err, "conjure: the current directory: {e}");
            return 2;
        }
    };
    let wire = |project, config: &_, attempt| live(project, env, config, attempt);
    main_with(args, &cwd, env, &wire, out, err)
}

/// [`main`] with the directory and the wiring given, so the tests can run a command with their
/// doubles.
#[doc(hidden)]
pub fn main_with(
    args: Vec<OsString>,
    cwd: &Path,
    env: &dyn Env,
    wire: Wire,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let code = match Cli::try_parse_from(args) {
        Ok(cli) => match run_command(cli, cwd, env, wire, out, err) {
            Ok(code) => code,
            Err(e) => {
                let _ = writeln!(err, "conjure: {e:#}");
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
    wire: Wire,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    // the server finds its project from the root the editor sends, not from here: the
    // other commands open the project holding the current directory
    let project = || Project::open(cwd);
    match cli.cmd {
        Some(Cmd::Lsp) => lsp::lsp(),
        None => conjure_in_project(&cli.run, cwd, project()?, wire, out, err),
        Some(Cmd::Doctor) => doctor::doctor(&project()?, env, &GrammarCache::user(env), out),
        Some(Cmd::Trust { revoke }) => trust::trust(&project()?, env, revoke, out),
        Some(Cmd::Cost { since, json }) => {
            let project = project()?;
            let offset = local_offset(&project, wire)?;
            let ledger = JsonlLedger::for_project(&project)?;
            cost::cost(&ledger, since.as_deref(), offset, json, out)
        }
        Some(Cmd::Log { n }) => {
            let project = project()?;
            let offset = local_offset(&project, wire)?;
            let store = JsonlChangeStore::for_project(&project)?;
            let n = n.map_or(log::DEFAULT_COUNT, |n| n as usize);
            log::show_log(&store, n, offset, out, err)
        }
    }
}

fn conjure_in_project(
    args: &RunArgs,
    cwd: &Path,
    project: Project,
    wire: Wire,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let config = conjure_config(&project)?;
    let attempt = if args.try_harder {
        Attempt::TryHarder
    } else {
        Attempt::First
    };
    run::conjure_here(args, cwd, project, &config, attempt, wire, out, err)
}

/// The user's time zone: the wiring's clock when it opens (the tests' doubles), else the
/// system's. These commands need no model, so a wiring that does not open is not an error.
fn local_offset(project: &Project, wire: Wire) -> Result<FixedOffset> {
    let config = conjure_config(project)?;
    Ok(match wire(project.clone(), &config, Attempt::First) {
        Ok(wired) => wired.clock.offset(),
        Err(_) => *Local::now().offset(),
    })
}
