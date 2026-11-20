//! What scry does, a module for each thing: review a change, list the
//! findings, explain one, dismiss one, and report what scry spent.

pub mod dismiss_finding;
pub mod explain_finding;
pub mod list_findings;
pub mod report_cost;
pub mod review_change;

use sorcerer_core::domain::versioned::Snapshot;

use crate::domain::findings::Findings;
use crate::ports::finding_store::FindingStore;

/// The findings as the store holds them now. No store yet is no findings;
/// a store that cannot be read is an error saying why.
pub(crate) fn read_findings(store: &dyn FindingStore) -> anyhow::Result<Findings> {
    Ok(match store.read()? {
        Snapshot::Current(stored) => stored,
        Snapshot::Absent => Findings::default(),
        Snapshot::Unreadable(why) => anyhow::bail!("the finding store cannot be read ({why})"),
    })
}
