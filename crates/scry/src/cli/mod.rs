//! The command line: `scry [FILES]...` runs a review and prints the open
//! findings in its scope, writing a run log (`scry log` prints it, `-v`
//! streams it); `scry doctor` says whether scry can run here. Each command
//! parses, calls one use case and prints.
//! Exit codes: 0 nothing open in scope, 1 open findings in scope, 2 could
//! not run.

mod cost;
mod dismiss;
mod doctor;
mod format;
mod list;
mod log;
mod review;
mod why;

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Local, Utc};
use clap::{Args, Parser, Subcommand};
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::wiring::{Wiring, LiveWiring};
use crate::domain::config::scry_config;
pub use doctor::doctor_rows;

#[derive(Parser, Debug)]
#[command(
    name = "scry",
    version,
    about = "Reads a change and says what it gets wrong",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Files to review (their changed units; with --all, every unit). A file
    /// named like a subcommand is passed as ./list
    files: Vec<PathBuf>,
    /// Every unit of each file in scope, not only the changed ones
    #[arg(long)]
    all: bool,
    /// The files the branch changed since it left REV
    #[arg(long, value_name = "REV", conflicts_with = "files")]
    since: Option<String>,
    /// The findings as JSON, one object a line
    #[arg(long)]
    json: bool,
    /// Look again at units already reviewed as they are now
    #[arg(long)]
    fresh: bool,
    /// The most this run may spend, in dollars (the day's cap still holds)
    #[arg(long, value_name = "USD")]
    budget: Option<f64>,
    /// Stream the run log to stderr as the run goes
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// The findings in the store, by status
    List {
        #[arg(long)]
        status: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// What scry spent, a local day a row
    Cost {
        #[arg(long, value_name = "YYYY-MM-DD")]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Stop showing a finding, here or everywhere
    Dismiss {
        id: String,
        #[arg(long)]
        everywhere: bool,
    },
    /// A finding in full
    Why { id: String },
    /// A run's log: the latest, or the Nth latest
    Log {
        /// 1 is the latest run, 2 the one before, …
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        n: Option<u32>,
        /// The logs kept, newest first: file, time, result
        #[arg(long, conflicts_with_all = ["n", "path"])]
        list: bool,
        /// The log's path only
        #[arg(long)]
        path: bool,
    },
    /// Whether scry can run here: provider, Jev, languages, steering, store, ledger
    Doctor,
}

/// The binary's entry: the process's directory, the live wiring, and the
/// terminal's width (100 when stdout is not a terminal).
pub fn main(args: Vec<OsString>, env: &dyn Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(err, "scry: the current directory: {e}");
            return 2;
        }
    };
    let width = if std::io::stdout().is_terminal() {
        terminal_size::terminal_size()
            .map(|(w, _)| w.0 as usize)
            .unwrap_or(100)
    } else {
        100
    };
    main_with(args, &cwd, env, out, err, width, |p| LiveWiring::open(p, env))
}

/// [`main`] with the directory, width and wiring given (the tests' doubles).
#[doc(hidden)]
pub fn main_with<W: Wiring>(
    args: Vec<OsString>,
    cwd: &Path,
    env: &dyn Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
    width: usize,
    wire: impl Fn(&Project) -> Result<W>,
) -> i32 {
    let line = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let text = e.render().to_string();
            let _ = if e.use_stderr() {
                err.write_all(text.as_bytes())
            } else {
                out.write_all(text.as_bytes())
            };
            return code;
        }
    };
    let code = match go(cli, &line, cwd, env, out, err, width, wire) {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(err, "scry: {e:#}");
            2
        }
    };
    let _ = out.flush();
    let _ = err.flush();
    code
}

#[allow(clippy::too_many_arguments)]
fn go<W: Wiring>(
    cli: Cli,
    line: &str,
    cwd: &Path,
    env: &dyn Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
    width: usize,
    wire: impl Fn(&Project) -> Result<W>,
) -> Result<i32> {
    let project = Project::open(cwd)?;
    match cli.cmd {
        None => {
            // off is not a failure, and needs no wiring (no Jev key, no
            // provider): a pre-commit hook lets the commit through
            if !scry_config(&project)?.enabled {
                writeln!(err, "scry: off ([scry] enabled = false)")?;
                return Ok(0);
            }
            review::logged(&cli.run, line, cwd, &project, wire, out, err, width)
        }
        Some(Cmd::Doctor) => doctor::doctor(&project, env, &GrammarCache::user(env), out),
        Some(Cmd::List { status, json }) => list::list(&project, &status, json, out),
        Some(Cmd::Cost { since, json }) => {
            let (_, offset) = clock_of(&project, &wire);
            cost::cost(&project, since.as_deref(), offset, json, out, err)
        }
        Some(Cmd::Dismiss { id, everywhere }) => {
            let (now, _) = clock_of(&project, &wire);
            dismiss::dismiss(&project, &id, everywhere, now, out, err)
        }
        Some(Cmd::Why { id }) => why::why(&project, env, &id, out),
        Some(Cmd::Log { n, list, path }) => {
            let (_, offset) = clock_of(&project, &wire);
            log::show_log(&project, n.unwrap_or(1), list, path, offset, out, err)
        }
    }
}

/// The time and local offset: the wiring's clock when it opens (the tests'
/// doubles), else the system's. These commands need no Jev, so a wiring
/// that does not open is not an error here.
fn clock_of<W: Wiring>(
    project: &Project,
    wire: &impl Fn(&Project) -> Result<W>,
) -> (DateTime<Utc>, FixedOffset) {
    match wire(project) {
        Ok(w) => {
            let d = w.deps();
            (d.clock.now(), d.clock.offset())
        }
        Err(_) => (Utc::now(), *Local::now().offset()),
    }
}
