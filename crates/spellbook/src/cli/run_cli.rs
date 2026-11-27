//! The program's entry: parse the arguments, find the project, run the command.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::wiring::{Wire, live};
use crate::domain::config::{SPELLBOOK_IS_OFF, spellbook_config};

use super::arguments::{Cli, Cmd};
use super::console::print_usage_or_error;
use super::file_arguments::relative_to_root;
use super::{check, doctor, fix, holes, list, lsp, new, show, sync, trust};

/// The program's entry: runs the command in the current directory, with the real files,
/// grammars and clock.
pub fn main(args: Vec<OsString>, env: &dyn Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            let _ = writeln!(err, "spellbook: the current directory: {e}");
            return 2;
        }
    };
    let wire = |project| live(project, env);
    main_with(args, &cwd, env, &wire, out, err)
}

/// [`main`] with the directory and the wiring given, so the tests can run a command on their
/// project with vendored grammars and a fixed clock.
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
        Ok(cli) => match run_command(cli.cmd, cwd, env, wire, out, err) {
            Ok(code) => code,
            Err(e) => {
                let _ = writeln!(err, "spellbook: {e:#}");
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
    cmd: Cmd,
    cwd: &Path,
    env: &dyn Env,
    wire: Wire,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    // the editor sends its root at initialize; the server opens the project itself
    if matches!(cmd, Cmd::Lsp) {
        return lsp::lsp(env);
    }
    let project = Project::open(cwd)?;
    let works_on_code = !matches!(cmd, Cmd::Trust { .. } | Cmd::Doctor);
    if works_on_code && !spellbook_config(&project)?.enabled {
        writeln!(err, "{SPELLBOOK_IS_OFF}")?;
        return Ok(2);
    }
    match cmd {
        Cmd::Lsp => unreachable!("served above"),
        Cmd::Trust { revoke } => trust::trust(&project, env, revoke, out),
        Cmd::Doctor => doctor::doctor(&project, env, out),
        Cmd::List => list::list(&wire(project)?, out),
        Cmd::Show { generator } => show::show(&wire(project)?, &generator, out, err),
        Cmd::New {
            generator,
            set,
            plan,
            json,
        } => {
            let request = new::NewRequest {
                generator,
                set,
                only_plan: plan,
                json,
            };
            new::new(&wire(project)?, &request, out, err)
        }
        Cmd::Holes { files, json } => {
            let files = relative_to_root(&project, cwd, &files)?;
            holes::holes(&wire(project)?, &files, json, out)
        }
        Cmd::Fix {
            at,
            rule,
            fix,
            set,
            plan,
            json,
        } => {
            let Some((file, line)) = at
                .rsplit_once(':')
                .and_then(|(file, line)| Some((file, line.parse::<u32>().ok()?)))
            else {
                writeln!(err, "spellbook: `{at}` is not FILE:LINE (a line from 1)")?;
                return Ok(2);
            };
            let file = relative_to_root(&project, cwd, &[PathBuf::from(file)])?;
            let request = fix::FixRequest {
                file: forward_slashes(&file[0]),
                line,
                rule,
                fix,
                set,
                only_plan: plan,
                json,
            };
            fix::fix(&wire(project)?, &request, out, err)
        }
        Cmd::Sync {
            init,
            check,
            plan,
            json,
        } => {
            let request = sync::SyncRequest {
                init,
                check,
                only_plan: plan,
                json,
            };
            sync::sync(&wire(project)?, &request, out, err)
        }
        Cmd::Check {
            files,
            since,
            baseline,
            json,
        } => {
            if baseline {
                return check::record_baseline(&wire(project)?, json, out);
            }
            let scope = match since {
                Some(rev) => check::CheckScope::Since(rev),
                None if files.is_empty() => check::CheckScope::Project,
                None => check::CheckScope::Files(relative_to_root(&project, cwd, &files)?),
            };
            check::check(&wire(project)?, &scope, json, out)
        }
    }
}
