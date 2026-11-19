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
        .filter(|f| req.statuses.is_empty() || req.statuses.contains(&status_word(f.status)))
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

/// Open first, the order of the status type.
fn status_rank(s: Status) -> u8 {
    use Status::*;
    match s {
        Open => 0,
        Waiting => 1,
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
