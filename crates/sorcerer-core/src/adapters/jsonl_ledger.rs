//! The ledger as a file, `.sorcerer/state/ledger.jsonl`: one line of JSON per paid call.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::domain::errand::{Errand, LedgerRows};
use crate::domain::project::Project;
use crate::ports::ledger::Ledger;
use crate::{Error, Result};

/// The ledger kept in one file of JSON lines.
#[derive(Debug, Clone)]
pub struct JsonlLedger {
    path: PathBuf,
}

impl JsonlLedger {
    /// The project's ledger, `.sorcerer/state/ledger.jsonl`.
    pub fn for_project(p: &Project) -> Result<JsonlLedger> {
        Ok(JsonlLedger {
            path: p.state_dir()?.join("ledger.jsonl"),
        })
    }

    /// A ledger kept in any file, for tests.
    pub fn at(path: PathBuf) -> JsonlLedger {
        JsonlLedger { path }
    }
}

impl Ledger for JsonlLedger {
    fn path(&self) -> &Path {
        &self.path
    }

    /// Writes the line in one write call, so lines from two processes never interleave.
    fn append(&self, errand: &Errand) -> Result<()> {
        let mut line = serde_json::to_vec(errand).map_err(|err| Error::Config {
            file: self.path.clone(),
            why: err.to_string(),
        })?;
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|err| Error::io(&self.path, err))?;
        file.write_all(&line)
            .map_err(|err| Error::io(&self.path, err))
    }

    /// A ledger file that is not there yet has no rows. A line that does not parse (a crash
    /// mid-write, a hand edit) is counted and skipped, so one bad line never hides the rest.
    fn read(&self) -> Result<LedgerRows> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(rows_of(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LedgerRows::default()),
            Err(e) => Err(Error::io(&self.path, e)),
        }
    }
}

fn rows_of(text: &str) -> LedgerRows {
    let lines = text.lines().filter(|line| !line.trim().is_empty());
    lines.fold(LedgerRows::default(), |mut rows, line| {
        match serde_json::from_str(line) {
            Ok(errand) => rows.errands.push(errand),
            Err(_) => rows.skipped += 1,
        }
        rows
    })
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;

    fn errand(at: &str, product: &str, cost: Option<f64>) -> Errand {
        Errand {
            at: at.parse().unwrap(),
            product: product.into(),
            verb: "review".into(),
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
        let l = JsonlLedger::at(d.path().join("ledger.jsonl"));
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
            JsonlLedger::at(d.path().join("none.jsonl"))
                .read()
                .unwrap()
                .errands
                .is_empty()
        );
    }

    #[test]
    fn a_malformed_line_is_skipped_and_counted() {
        let d = tempfile::TempDir::new().unwrap();
        let l = JsonlLedger::at(d.path().join("ledger.jsonl"));
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
        let l = JsonlLedger::at(d.path().join("ledger.jsonl"));
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
