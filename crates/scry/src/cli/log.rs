//! `scry log`: prints the logs scry's runs write, or lists them.

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::FixedOffset;
use sorcerer_core::domain::project::Project;

use super::Console;
use super::format::table;
use crate::adapters::markdown_run_log;
use crate::domain::log_text::count;

/// What `scry log` is asked for. `n` counts back from the latest run: 1 is
/// the latest, 2 the one before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LogRequest {
    /// The `n`th latest run's log.
    Text(u32),
    /// Where the `n`th latest run's log is (`--path`).
    Path(u32),
    /// Every log kept, newest first, with its time and result (`--list`).
    List,
}

/// Prints what the request asks for. With no log yet, or no `n`th one,
/// it says so on stderr and exits 2. `offset` is the user's time zone, for
/// the times in the list.
pub(super) fn show_log(
    project: &Project,
    request: LogRequest,
    offset: FixedOffset,
    console: &mut Console,
) -> Result<i32> {
    let dir = markdown_run_log::runs_dir(&project.state);
    let logs = markdown_run_log::runs(&dir);
    if logs.is_empty() {
        write!(console.err, "{}", no_log_yet_text(&dir))?;
        return Ok(2);
    }
    let n = match request {
        LogRequest::List => {
            write!(console.out, "{}", logs_table_text(&logs, offset))?;
            return Ok(0);
        }
        LogRequest::Text(n) | LogRequest::Path(n) => n,
    };
    let Some(log) = logs.get(n as usize - 1) else {
        write!(console.err, "{}", no_such_log_text(n, logs.len()))?;
        return Ok(2);
    };
    if let LogRequest::Path(_) = request {
        writeln!(console.out, "{}", log.display())?;
    } else {
        let text =
            std::fs::read_to_string(log).map_err(|e| anyhow::anyhow!("{}: {e}", log.display()))?;
        console.out.write_all(text.as_bytes())?;
    }
    Ok(0)
}

// The command's text.

fn no_log_yet_text(dir: &Path) -> String {
    format!(
        "scry: no run log yet (a run writes one in {})\n",
        dir.display()
    )
}

fn no_such_log_text(n: u32, kept: usize) -> String {
    format!("scry: no run log {n}: {} kept\n", count(kept, "log"))
}

/// A table with a row for each run log: its file name, when the run was
/// (in the user's time zone) and its result line.
fn logs_table_text(logs: &[PathBuf], offset: FixedOffset) -> String {
    let rows: Vec<Vec<String>> = logs
        .iter()
        .map(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let time = markdown_run_log::run_time(&name)
                .map(|t| {
                    t.with_timezone(&offset)
                        .format("%Y-%m-%d %H:%M:%S %:z")
                        .to_string()
                })
                .unwrap_or_default();
            let text = std::fs::read_to_string(p).unwrap_or_default();
            let result = markdown_run_log::result_of(&text)
                .unwrap_or("(no result line)")
                .to_string();
            vec![name, time, result]
        })
        .collect();
    table(&["log", "time", "result"], &rows)
}
