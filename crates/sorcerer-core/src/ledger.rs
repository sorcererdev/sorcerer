//! The ledger: `.sorcerer/state/ledger.jsonl`, one JSON line per paid
//! call (an errand) of every product. Append-only; caps and `cost`
//! commands read it.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Error, Project, Result};

/// One paid model call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Errand {
    pub at: DateTime<Utc>,
    /// `scry`, `conjure`, …
    pub product: String,
    /// `find`, `check`, `decide`, …
    pub verb: String,
    pub model: String,
    pub provider: String,
    pub cost_usd: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub seconds: f64,
    pub ok: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Ledger {
    path: PathBuf,
}

#[derive(Debug, Default)]
pub struct LedgerRows {
    pub errands: Vec<Errand>,
    /// Lines that did not parse (a crash mid-write, a hand edit).
    pub skipped: usize,
}

impl Ledger {
    /// The project's ledger, `<state>/ledger.jsonl`.
    pub fn for_project(p: &Project) -> Result<Ledger> {
        Ok(Ledger {
            path: p.state_dir()?.join("ledger.jsonl"),
        })
    }

    pub fn at(path: PathBuf) -> Ledger {
        Ledger { path }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Appends one line in one write, so lines from two processes never interleave.
    pub fn append(&self, e: &Errand) -> Result<()> {
        let mut line = serde_json::to_vec(e).map_err(|err| Error::Config {
            file: self.path.clone(),
            why: err.to_string(),
        })?;
        line.push(b'\n');
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|err| Error::io(&self.path, err))?;
        f.write_all(&line).map_err(|err| Error::io(&self.path, err))
    }

    pub fn read(&self) -> Result<LedgerRows> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LedgerRows::default()),
            Err(e) => return Err(Error::io(&self.path, e)),
        };
        let mut rows = LedgerRows::default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            match serde_json::from_str(line) {
                Ok(e) => rows.errands.push(e),
                Err(_) => rows.skipped += 1,
            }
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errand(at: &str, product: &str, cost: Option<f64>) -> Errand {
        Errand {
            at: at.parse().unwrap(),
            product: product.into(),
            verb: "find".into(),
            model: "sonnet".into(),
            provider: "claude".into(),
            cost_usd: cost,
            input_tokens: 10,
            output_tokens: 5,
            seconds: 1.5,
            ok: true,
            note: None,
        }
    }

    #[test]
    fn append_then_read_round_trips() {
        let d = tempfile::TempDir::new().unwrap();
        let l = Ledger::at(d.path().join("ledger.jsonl"));
        let e = errand("2026-11-17T10:00:00Z", "scry", Some(0.04));
        l.append(&e).unwrap();
        l.append(&e).unwrap();
        let rows = l.read().unwrap();
        assert_eq!(rows.errands, vec![e.clone(), e]);
        assert_eq!(rows.skipped, 0);
    }

    #[test]
    fn a_missing_ledger_is_empty() {
        let d = tempfile::TempDir::new().unwrap();
        assert!(
            Ledger::at(d.path().join("none.jsonl"))
                .read()
                .unwrap()
                .errands
                .is_empty()
        );
    }

    #[test]
    fn a_malformed_line_is_skipped_and_counted() {
        let d = tempfile::TempDir::new().unwrap();
        let l = Ledger::at(d.path().join("ledger.jsonl"));
        l.append(&errand("2026-11-17T10:00:00Z", "scry", None))
            .unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(d.path().join("ledger.jsonl"))
            .unwrap()
            .write_all(b"{oops\n")
            .unwrap();
        let rows = l.read().unwrap();
        assert_eq!((rows.errands.len(), rows.skipped), (1, 1));
    }

    #[test]
    fn two_threads_appending_give_whole_lines() {
        let d = tempfile::TempDir::new().unwrap();
        let l = Ledger::at(d.path().join("ledger.jsonl"));
        std::thread::scope(|s| {
            for _ in 0..2 {
                s.spawn(|| {
                    for _ in 0..500 {
                        l.append(&errand("2026-11-17T10:00:00Z", "scry", Some(0.01)))
                            .unwrap();
                    }
                });
            }
        });
        let rows = l.read().unwrap();
        assert_eq!((rows.errands.len(), rows.skipped), (1000, 0));
    }
}
