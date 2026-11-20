//! The stored findings for `scry list`: open ones first, then by file and
//! line.

use crate::domain::finding::{Finding, Status, status_word};
use crate::ports::finding_store::FindingStore;
use crate::usecases::read_findings;

/// Which findings to list: those with any of these statuses (`open`,
/// `dismissed-here`, ...), or every finding when none are given.
#[derive(Debug, Clone, Default)]
pub struct ListRequest {
    pub statuses: Vec<String>,
}

/// The findings the request asks for, sorted by status (open first, then
/// the order the statuses are declared in), then by file, then by line.
pub fn list_findings(
    store: &dyn FindingStore,
    request: &ListRequest,
) -> anyhow::Result<Vec<Finding>> {
    let stored = read_findings(store)?;
    let asked_for = |f: &Finding| {
        request.statuses.is_empty()
            || request
                .statuses
                .iter()
                .any(|asked| status_named(asked, f.status))
    };
    let mut listed: Vec<Finding> = stored.findings.into_iter().filter(asked_for).collect();
    listed.sort_by_key(|f| {
        (
            status_rank(f.status),
            f.place.file.clone(),
            f.place.lines.start,
        )
    });
    Ok(listed)
}

/// Whether the status word the user gave names this status. `waiting`, the
/// old word for `unverified`, still works: `scry list --status waiting`
/// lists the unverified findings.
fn status_named(asked: &str, status: Status) -> bool {
    let asked = if asked == "waiting" {
        "unverified"
    } else {
        asked
    };
    asked == status_word(status)
}

/// Where a status sorts: open first, then the order of [`Status`].
fn status_rank(status: Status) -> u8 {
    use Status::*;
    match status {
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
