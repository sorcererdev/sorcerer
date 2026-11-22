//! The command line. `conjure FILE:LINE[-END]` conjures one change at that place and prints it
//! (the goal, each file's diff and whether it parses); it lands nothing, since only an editor
//! can land a change. `--finding ID` conjures for a scry finding, `--try-harder` asks the
//! stronger model. `conjure lsp` is the editor's language server, which lands changes.
//!
//! Each command reads its arguments, calls one use case and prints what comes back. The exit
//! code is 0 when a change was conjured (or none was needed), and 2 when conjure could not run.

mod cost;
mod doctor;
mod format;
mod log;
mod lsp;
mod run;
mod trust;

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use chrono::{FixedOffset, Local};
use clap::{Args, Parser, Subcommand};
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::jsonl_change_store::JsonlChangeStore;
use crate::adapters::wiring::{Wire, live};
pub use doctor::{DoctorRow, doctor_rows};

use crate::domain::config::conjure_config;
use crate::domain::record::Attempt;

#[derive(Parser, Debug)]
#[command(
    name = "conjure",
    version,
    about = "Writes one small change where you ask",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Whether conjure can run here: git, provider, settings, languages, scry's findings, records
    Doctor,
    /// What conjure spent, a local day a row
    Cost {
        #[arg(long, value_name = "YYYY-MM-DD")]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// The last changes the editor recorded, newest first
    Log {
        /// How many (10 by default)
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        n: Option<u32>,
    },
    /// Let the editor conjure in this project although its config runs code or moves your key
    Trust {
        /// Take the project off the trust list instead
        #[arg(long)]
        revoke: bool,
    },
    /// The editor's language server, over standard input and output
    Lsp,
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Where to conjure: a file and a line, or a range of lines (src/cart.py:12, src/cart.py:12-20)
    #[arg(value_name = "FILE:LINE[-END]")]
    target: Option<String>,
    /// Conjure for this scry finding
    #[arg(long, value_name = "ID")]
    finding: Option<String>,
    /// Ask the stronger model, which may read the project first
    #[arg(long)]
    try_harder: bool,
}

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

/// When the arguments do not parse: `--help` and `--version` print on stdout and exit 0; a
/// mistake prints on stderr and exits 2.
fn print_usage_or_error(e: &clap::Error, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let text = e.render().to_string();
    if e.use_stderr() {
        let _ = err.write_all(text.as_bytes());
        2
    } else {
        let _ = out.write_all(text.as_bytes());
        0
    }
}

fn run_command(
    cli: Cli,
    cwd: &Path,
    env: &dyn Env,
    wire: Wire,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    if let Some(Cmd::Lsp) = cli.cmd {
        // the server finds its project from the root the editor sends, not from here
        return lsp::lsp();
    }
    let project = Project::open(cwd)?;
    match cli.cmd {
        Some(Cmd::Lsp) => lsp::lsp(),
        None => conjure_in_project(&cli.run, cwd, project, wire, out, err),
        Some(Cmd::Doctor) => doctor::doctor(&project, env, &GrammarCache::user(env), out),
        Some(Cmd::Trust { revoke }) => trust::trust(&project, env, revoke, out),
        Some(Cmd::Cost { since, json }) => {
            let offset = local_offset(&project, wire)?;
            let ledger = JsonlLedger::for_project(&project)?;
            cost::cost(&ledger, since.as_deref(), offset, json, out)
        }
        Some(Cmd::Log { n }) => {
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
