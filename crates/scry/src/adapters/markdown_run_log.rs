//! The run log, kept as Markdown files under `.sorcerer/state/scry/runs/`,
//! the newest 20.
//!
//! A run's log is written by two hands. The review writes its part through
//! the `RunLog` port: the scope, the review calls, what became of each
//! finding, the verification rounds, what it remembered. The command line
//! adds the head before it and the summary after, streams it all to
//! stderr under `-v` ([`StreamingLog`]), and writes the file at the end.

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::ports::run_log::RunLog;

/// The run log as a run writes it: kept whole for the file, and under
/// `-v` also written to stderr as it grows. The command's other stderr
/// lines go through it too, so the log and those lines never come out of
/// order.
pub struct StreamingLog<'w> {
    text: RefCell<String>,
    err: RefCell<&'w mut dyn Write>,
    verbose: bool,
}

impl<'w> StreamingLog<'w> {
    /// An empty log. With `verbose`, what is put in it also goes to `err`.
    pub fn new(err: &'w mut dyn Write, verbose: bool) -> StreamingLog<'w> {
        StreamingLog {
            text: RefCell::new(String::new()),
            err: RefCell::new(err),
            verbose,
        }
    }

    /// The log so far.
    pub fn text(&self) -> String {
        self.text.borrow().clone()
    }

    /// Writes one line on stderr, not in the log.
    pub fn say(&self, line: &str) -> std::io::Result<()> {
        writeln!(self.err.borrow_mut(), "{line}")
    }

    /// Writes the bytes on stderr, not in the log.
    pub fn write_err(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.err.borrow_mut().write_all(bytes)
    }
}

impl RunLog for StreamingLog<'_> {
    fn put(&self, text: &str) {
        self.text.borrow_mut().push_str(text);
        if self.verbose {
            let _ = self.err.borrow_mut().write_all(text.as_bytes());
        }
    }
}

/// How many run logs are kept: 20.
pub const KEEP: usize = 20;

/// What starts a log's result line; `scry log --list` prints what follows
/// it.
pub const RESULT: &str = "**Result:** ";

/// How a log's name writes the time the run started, in UTC.
const STAMP_FORMAT: &str = "%Y-%m-%dT%H-%M-%SZ";

/// Where the run logs are, under the project's state directory.
pub fn runs_dir(state: &Path) -> PathBuf {
    state.join("scry").join("runs")
}

/// Writes the text as a new log, then deletes all but the newest 20, and
/// gives back the new log's path. The log is named for the time the run
/// started; when two runs start in the same second, the second log gets
/// `-2`, the third `-3`, and so on. An existing log is never overwritten.
///
/// Example: `2026-11-18T23-00-00Z.md`, then `2026-11-18T23-00-00Z-2.md`.
pub fn write_run(dir: &Path, at: DateTime<Utc>, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = at.format(STAMP_FORMAT).to_string();
    let path = write_new_file(dir, &stamp, 1, text)?;
    for old in runs(dir).into_iter().skip(KEEP) {
        std::fs::remove_file(old)?;
    }
    Ok(path)
}

/// Writes the text to the first log name from `counter` on that no file
/// has yet.
fn write_new_file(dir: &Path, stamp: &str, counter: u32, text: &str) -> std::io::Result<PathBuf> {
    let path = dir.join(log_name(stamp, counter));
    let created = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path);
    match created {
        Ok(mut file) => {
            file.write_all(text.as_bytes())?;
            Ok(path)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            write_new_file(dir, stamp, counter + 1, text)
        }
        Err(e) => Err(e),
    }
}

/// `2026-11-18T23-00-00Z.md` for the first log of that second, then
/// `…Z-2.md`, `…Z-3.md`.
fn log_name(stamp: &str, counter: u32) -> String {
    if counter == 1 {
        format!("{stamp}.md")
    } else {
        format!("{stamp}-{counter}.md")
    }
}

/// The run logs in the directory, newest first: by the time in the name,
/// then by the counter, so `-10` comes before `-9`. Other files are not
/// logs and are left alone.
pub fn runs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut logs: Vec<(RunKey, PathBuf)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            run_key(&name).map(|key| (key, entry.path()))
        })
        .collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    logs.into_iter().map(|(_, path)| path).collect()
}

/// What a log's name says, in the order logs sort by: the UTC time stamp,
/// then the counter for runs that started in the same second.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RunKey {
    stamp: String,
    counter: u32,
}

/// What a log's name says; none when the name is not a log's.
///
/// Example: `2026-11-18T23-00-00Z.md` is counter 1, and
/// `2026-11-18T23-00-00Z-3.md` counter 3; `notes.md` is not a log.
fn run_key(name: &str) -> Option<RunKey> {
    let stem = name.strip_suffix(".md")?;
    let (stamp, counter) = match stem.split_once("Z-") {
        Some((time, counter)) => (format!("{time}Z"), counter.parse().ok()?),
        None => (stem.to_string(), 1),
    };
    let is_stamp = stamp.len() == 20
        && stamp.ends_with('Z')
        && chrono::NaiveDateTime::parse_from_str(&stamp, STAMP_FORMAT).is_ok();
    is_stamp.then_some(RunKey { stamp, counter })
}

/// When the run started, read from its log's name.
pub fn run_time(name: &str) -> Option<DateTime<Utc>> {
    let key = run_key(name)?;
    chrono::NaiveDateTime::parse_from_str(&key.stamp, STAMP_FORMAT)
        .ok()
        .map(|time| time.and_utc())
}

/// The log's result line, without its lead: what follows [`RESULT`] on the
/// last line that starts with it.
pub fn result_of(text: &str) -> Option<&str> {
    text.lines()
        .rev()
        .find_map(|line| line.strip_prefix(RESULT))
}
