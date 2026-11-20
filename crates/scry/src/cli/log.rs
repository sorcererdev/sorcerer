//! `scry log`: reads the run logs the run writes.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::FixedOffset;
use sorcerer_core::domain::project::Project;

use super::format::table;
use crate::adapters::markdown_run_log;
use crate::domain::log_text::count;

/// `scry log`: the Nth latest run's log, the list of them, or a path.
pub(super) fn show_log(
    project: &Project,
    n: u32,
    list: bool,
    path: bool,
    offset: FixedOffset,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let dir = markdown_run_log::runs_dir(&project.state);
    let logs = markdown_run_log::runs(&dir);
    if logs.is_empty() {
        write!(err, "{}", no_log_yet_text(&dir))?;
        return Ok(2);
    }
    if list {
        write!(out, "{}", logs_table_text(&logs, offset))?;
        return Ok(0);
    }
    let Some(p) = logs.get(n as usize - 1) else {
        write!(err, "{}", no_such_log_text(n, logs.len()))?;
        return Ok(2);
    };
    if path {
        writeln!(out, "{}", p.display())?;
    } else {
        let text =
            std::fs::read_to_string(p).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
        out.write_all(text.as_bytes())?;
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

/// A row a run log: its name, its time in OFFSET and its result line.
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
