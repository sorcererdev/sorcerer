//! The program's entry: parse the arguments, find the project, run the command.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Local, Utc};
use clap::Parser;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use super::arguments::{Cli, Cmd, RunArgs};
use super::console::{Console, print_usage_or_error, terminal_width};
use super::format::Output;
use super::log::{self, LogRequest};
use super::{cost, dismiss, doctor, list, lsp, review, trust, why};
use crate::adapters::wiring::{LiveWiring, Wiring};
use crate::domain::config::scry_config;
use crate::domain::finding::Dismissal;

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
pub(super) fn clock_of<W: Wiring>(
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
