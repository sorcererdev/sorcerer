//! The run log as Markdown files: a use case writes its part through the
//! `RunLog` port (`review_change`: the scope, the review calls, each
//! finding's path, the rounds, what it remembered); the command line adds
//! the run's head and the summary, streams it all to stderr with `-v`
//! ([`StreamingLog`]), and keeps it under `state/scry/runs/`, the newest
//! [`KEEP`].

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::ports::run_log::RunLog;

/// The run log as a run writes it: kept whole for the file, and with
/// `-v` streamed to ERR as it grows. Stderr's other lines go through it
/// too, so the two never interleave out of order.
pub struct StreamingLog<'w> {
    text: RefCell<String>,
    err: RefCell<&'w mut dyn Write>,
    verbose: bool,
}

impl<'w> StreamingLog<'w> {
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

    /// One line on stderr, not in the log.
    pub fn say(&self, s: &str) -> std::io::Result<()> {
        writeln!(self.err.borrow_mut(), "{s}")
    }

    /// BYTES on stderr, not in the log.
    pub fn write_err(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.err.borrow_mut().write_all(bytes)
    }
}

impl RunLog for StreamingLog<'_> {
    fn put(&self, s: &str) {
        self.text.borrow_mut().push_str(s);
        if self.verbose {
            let _ = self.err.borrow_mut().write_all(s.as_bytes());
        }
    }
}

/// How many run logs are kept.
pub const KEEP: usize = 20;

/// The summary line's lead: `scry log --list` prints what follows it.
pub const RESULT: &str = "**Result:** ";

/// The run logs' directory under PROJECT's state.
pub fn runs_dir(state: &Path) -> PathBuf {
    state.join("scry").join("runs")
}

/// Writes TEXT as a new log in DIR named for AT (`2026-11-18T23-00-00Z.md`,
/// then `-2`, `-3` on a clash), then deletes all but the newest KEEP.
pub fn write_run(dir: &Path, at: DateTime<Utc>, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = at.format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let mut n = 1;
    let path = loop {
        let name = if n == 1 {
            format!("{stamp}.md")
        } else {
            format!("{stamp}-{n}.md")
        };
        let p = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p)
        {
            Ok(mut f) => {
                f.write_all(text.as_bytes())?;
                break p;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    for old in runs(dir).into_iter().skip(KEEP) {
        std::fs::remove_file(old)?;
    }
    Ok(path)
}

/// The run logs in DIR, newest first (by the time in the name, then the
/// clash counter). Other files are not logs and are left alone.
pub fn runs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut logs: Vec<((String, u32), PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            run_key(&name).map(|k| (k, e.path()))
        })
        .collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    logs.into_iter().map(|(_, p)| p).collect()
}

/// A log's name as (UTC stamp, clash counter): `…Z.md` is 1, `…Z-3.md` is 3.
pub fn run_key(name: &str) -> Option<(String, u32)> {
    let stem = name.strip_suffix(".md")?;
    let (stamp, n) = match stem.split_once("Z-") {
        Some((s, n)) => (format!("{s}Z"), n.parse().ok()?),
        None => (stem.to_string(), 1),
    };
    let ok = stamp.len() == 20
        && stamp.ends_with('Z')
        && chrono::NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%dT%H-%M-%SZ").is_ok();
    ok.then_some((stamp, n))
}

/// The time a log's name holds.
pub fn run_time(name: &str) -> Option<DateTime<Utc>> {
    let (stamp, _) = run_key(name)?;
    chrono::NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%dT%H-%M-%SZ")
        .ok()
        .map(|t| t.and_utc())
}

/// The log's one-line summary: what follows [`RESULT`] on its last such line.
pub fn result_of(text: &str) -> Option<&str> {
    text.lines().rev().find_map(|l| l.strip_prefix(RESULT))
}
