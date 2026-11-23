//! The program's entry: parse the arguments, find the project, run the command.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use clap::Parser;
use sorcerer_core::ports::env::Env;

use super::arguments::Cli;
use super::console::print_usage_or_error;

/// The program's entry: runs the command in the current directory.
pub fn main(args: Vec<OsString>, env: &dyn Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            let _ = writeln!(err, "spellbook: the current directory: {e}");
            return 2;
        }
    };
    main_with(args, &cwd, env, out, err)
}

/// [`main`] with the directory given, so the tests can run a command in a project of theirs.
#[doc(hidden)]
pub fn main_with(
    args: Vec<OsString>,
    _cwd: &Path,
    _env: &dyn Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let code = match Cli::try_parse_from(args) {
        Ok(_cli) => 0,
        Err(e) => print_usage_or_error(&e, out, err),
    };
    let _ = out.flush();
    let _ = err.flush();
    code
}
