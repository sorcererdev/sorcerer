//! Dismissing a finding, here or everywhere.

use chrono::{DateTime, Utc};

use crate::domain::finding::FindingId;
use crate::ports::finding_store::FindingStore;

/// Dismisses the finding ID (or a unique prefix of four or more) AT; with
/// EVERYWHERE, in every unit. The inner error says the id matched none or
/// more than one; the store is then unchanged.
pub fn dismiss_finding(
    store: &dyn FindingStore,
    at: DateTime<Utc>,
    id: &str,
    everywhere: bool,
) -> anyhow::Result<Result<FindingId, String>> {
    let mut outcome = Err(String::new());
    store.update(&mut |s| {
        outcome = s
            .dismiss(id, everywhere, at)
            .map(|()| s.get(id).map(|f| f.id.clone()).expect("just dismissed"));
    })?;
    Ok(outcome)
}
