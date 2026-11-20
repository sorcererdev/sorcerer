//! The command line. `scry [FILES]...` reviews a change and prints the
//! open findings in it, writing a run log as it goes (`scry log` prints
//! the log afterwards, `-v` streams it as the run goes). The other
//! commands look at what scry has stored: `list`, `why`, `dismiss`, `cost`,
//! `log`, and `doctor`, which says whether scry can run here. `lsp` is the
//! editor's language server; `trust` lets it review a project whose config
//! runs code or moves the API key.
//!
//! Each command reads its arguments, calls one use case and prints what
//! comes back. The exit code is 0 when nothing is open in the change, 1
//! when findings are, and 2 when scry could not run.

mod cost;
mod dismiss;
mod doctor;
mod format;
mod list;
mod log;
mod lsp;
mod review;
mod trust;
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

use crate::adapters::wiring::{LiveWiring, Wiring};
use crate::domain::config::scry_config;
use crate::domain::finding::Dismissal;
pub use doctor::{DoctorRow, doctor_rows};
use format::Output;
use log::LogRequest;

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
    /// The editor's language server, over standard input and output
    Lsp,
    /// Let the editor review this project although its config runs code or moves your key
    Trust {
        /// Take the project off the trust list instead
        #[arg(long)]
        revoke: bool,
    },
}

/// The program's entry: runs the command in the current directory, with
/// the real model calls, files and clock.
pub fn main(args: Vec<OsString>, env: &dyn Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(err, "scry: the current directory: {e}");
            return 2;
        }
    };
    main_with(args, &cwd, env, out, err, terminal_width(), |p| {
        LiveWiring::open(p, env)
    })
}

/// The terminal's width in columns; 100 when stdout is not a terminal, or
/// its width cannot be read.
fn terminal_width() -> usize {
    let terminal = std::io::stdout()
        .is_terminal()
        .then(terminal_size::terminal_size);
    terminal
        .flatten()
        .map_or(100, |(width, _)| usize::from(width.0))
}

/// [`main`] with the directory, width and wiring given, so the tests can
/// run a command with their doubles. The command line as typed is kept for
/// the run log.
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
    let mut console = Console { out, err, width };
    let code = match Cli::try_parse_from(args) {
        Ok(cli) => match run_command(cli, &line, cwd, env, &mut console, wire) {
            Ok(code) => code,
            Err(e) => {
                let _ = writeln!(console.err, "scry: {e:#}");
                2
            }
        },
        Err(e) => print_usage_or_error(&e, &mut console),
    };
    let _ = console.out.flush();
    let _ = console.err.flush();
    code
}

/// Where a command writes: its output (`out`), its messages and errors
/// (`err`), and how many columns wide the terminal is.
struct Console<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
    width: usize,
}

/// When the arguments do not parse: `--help` and `--version` print on
/// stdout and exit 0; a mistake prints on stderr and exits 2.
fn print_usage_or_error(e: &clap::Error, console: &mut Console) -> i32 {
    let text = e.render().to_string();
    if e.use_stderr() {
        let _ = console.err.write_all(text.as_bytes());
        2
    } else {
        let _ = console.out.write_all(text.as_bytes());
        0
    }
}

/// Runs the command the arguments name, in the project holding `cwd`, and
/// gives back its exit code.
fn run_command<W: Wiring>(
    cli: Cli,
    line: &str,
    cwd: &Path,
    env: &dyn Env,
    console: &mut Console,
    wire: impl Fn(&Project) -> Result<W>,
) -> Result<i32> {
    match cli.cmd {
        // the server finds its project from the root the editor sends, not from here
        Some(Cmd::Lsp) => lsp::lsp(),
        cmd => run_in_project(cmd, &cli.run, line, cwd, env, console, wire),
    }
}

/// Runs a command in the project holding `cwd`.
fn run_in_project<W: Wiring>(
    cmd: Option<Cmd>,
    run: &RunArgs,
    line: &str,
    cwd: &Path,
    env: &dyn Env,
    console: &mut Console,
    wire: impl Fn(&Project) -> Result<W>,
) -> Result<i32> {
    let project = Project::open(cwd)?;
    match cmd {
        Some(Cmd::Lsp) => lsp::lsp(),
        None => {
            // off is not a failure, and needs no wiring (no Jev key, no
            // provider): a pre-commit hook lets the commit through
            if !scry_config(&project)?.enabled {
                writeln!(console.err, "scry: off ([scry] enabled = false)")?;
                return Ok(0);
            }
            review::logged(run, line, cwd, &project, wire, console)
        }
        Some(Cmd::Doctor) => doctor::doctor(&project, env, &GrammarCache::user(env), console.out),
        Some(Cmd::List { status, json }) => {
            list::list(&project, &status, Output::json_if(json), console.out)
        }
        Some(Cmd::Cost { since, json }) => {
            let (_, offset) = clock_of(&project, &wire);
            let output = Output::json_if(json);
            cost::cost(&project, since.as_deref(), offset, output, console)
        }
        Some(Cmd::Dismiss { id, everywhere }) => {
            let (now, _) = clock_of(&project, &wire);
            let dismissal = if everywhere {
                Dismissal::Everywhere
            } else {
                Dismissal::Here
            };
            dismiss::dismiss(&project, &id, dismissal, now, console)
        }
        Some(Cmd::Why { id }) => why::why(&project, env, &id, console.out),
        Some(Cmd::Trust { revoke }) => trust::trust(&project, env, revoke, console.out),
        Some(Cmd::Log { n, list, path }) => {
            let (_, offset) = clock_of(&project, &wire);
            let n = n.unwrap_or(1);
            let request = match (list, path) {
                (true, _) => LogRequest::List,
                (false, true) => LogRequest::Path(n),
                (false, false) => LogRequest::Text(n),
            };
            log::show_log(&project, request, offset, console)
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
