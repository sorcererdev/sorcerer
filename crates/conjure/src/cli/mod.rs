//! The command line. `conjure FILE:LINE[-END]` conjures one change at that place and prints it
//! (the goal, each file's diff and whether it parses); it lands nothing, since only an editor
//! can land a change. `--finding ID` conjures for a scry finding, `--try-harder` asks the
//! stronger model.
//!
//! Each command reads its arguments, calls one use case and prints what comes back. The exit
//! code is 0 when a change was conjured (or none was needed), and 2 when conjure could not run.

mod run;

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use clap::{Args, Parser};
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::wiring::{Wire, live};
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
    #[command(flatten)]
    run: RunArgs,
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
    _env: &dyn Env,
    wire: Wire,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let code = match Cli::try_parse_from(args) {
        Ok(cli) => match run_command(cli, cwd, wire, out, err) {
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
    wire: Wire,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let project = Project::open(cwd)?;
    let config = conjure_config(&project)?;
    let attempt = if cli.run.try_harder {
        Attempt::TryHarder
    } else {
        Attempt::First
    };
    run::conjure_here(&cli.run, cwd, project, &config, attempt, wire, out, err)
}
