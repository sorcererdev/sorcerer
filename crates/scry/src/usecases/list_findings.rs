//! The findings in the store, by status: open first, then by file and line.

use crate::domain::finding::{Finding, Status, status_word};
use crate::ports::finding_store::FindingStore;
use crate::usecases::read_findings;

/// Which findings to list: those of STATUSES (`open`, `dismissed-here`, …),
/// all when none.
#[derive(Debug, Clone, Default)]
pub struct ListRequest {
    pub statuses: Vec<String>,
}

pub fn list_findings(store: &dyn FindingStore, req: &ListRequest) -> anyhow::Result<Vec<Finding>> {
    let st = read_findings(store)?;
    let mut fs: Vec<Finding> = st
        .findings
        .into_iter()
        .filter(|f| req.statuses.is_empty() || req.statuses.iter().any(|s| names(s, f.status)))
        .collect();
    fs.sort_by(|a, b| {
        (status_rank(a.status), &a.place.file, a.place.lines.start).cmp(&(
            status_rank(b.status),
            &b.place.file,
            b.place.lines.start,
        ))
    });
    Ok(fs)
}

/// Whether the status word ASKED names STATUS. `waiting`, the old word for
/// `unverified`, still works: `scry list --status waiting` lists the
/// unverified findings.
fn names(asked: &str, status: Status) -> bool {
    let asked = if asked == "waiting" {
        "unverified"
    } else {
        asked
    };
    asked == status_word(status)
}

/// Open first, the order of the status type.
fn status_rank(s: Status) -> u8 {
    use Status::*;
    match s {
        Open => 0,
        Unverified => 1,
        Withheld => 2,
        Refuted => 3,
        Unsure => 4,
        Elsewhere => 5,
        DismissedHere => 6,
        DismissedEverywhere => 7,
        Resolved => 8,
        Gone => 9,
    }
}
