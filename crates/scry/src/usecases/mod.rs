//! What scry does, one module a feature.

pub mod check_setup;
pub mod dismiss_finding;
pub mod explain_finding;
pub mod list_findings;
pub mod report_cost;
pub mod review_change;

use sorcerer_core::adapters::json_store::Snapshot;

use crate::domain::findings::Findings;
use crate::ports::finding_store::FindingStore;

/// The store as it is; absent is empty, unreadable an error saying why.
pub(crate) fn read_findings(store: &dyn FindingStore) -> anyhow::Result<Findings> {
    Ok(match store.read()? {
        Snapshot::Current(s) => s,
        Snapshot::Absent => Findings::default(),
        Snapshot::Unreadable(why) => anyhow::bail!("the finding store cannot be read ({why})"),
    })
}
