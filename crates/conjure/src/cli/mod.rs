//! The command line. `conjure --version` for now; the default run and the subcommands arrive
//! with the tasks that follow.

use std::ffi::OsString;
use std::io::Write;

use clap::Parser;
use sorcerer_core::ports::env::Env;

#[derive(Parser, Debug)]
#[command(
    name = "conjure",
    version,
    about = "Writes one small change where you ask"
)]
struct Cli {}

/// The program's entry: runs the command in the current directory.
pub fn main(args: Vec<OsString>, _env: &dyn Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match Cli::try_parse_from(args) {
        Ok(_) => 0,
        Err(e) => {
            let text = e.render().to_string();
            if e.use_stderr() {
                let _ = err.write_all(text.as_bytes());
                2
            } else {
                let _ = out.write_all(text.as_bytes());
                0
            }
        }
    }
}
