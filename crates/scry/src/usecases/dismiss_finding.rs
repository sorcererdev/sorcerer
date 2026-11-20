//! Dismissing a finding, here or everywhere.

use chrono::{DateTime, Utc};

use crate::domain::finding::{Dismissal, FindingId};
use crate::ports::finding_store::FindingStore;

/// Dismisses a finding, here or everywhere, and gives back its full id.
/// The id may be the start of one, at least four characters long. When it
/// matches no finding, or more than one, the inner error says so and the
/// store is unchanged.
///
/// Example: `dismiss_finding(store, now, "3fa9", Dismissal::Here)` dismisses
/// finding `3fa9c0` in its unit and gives back `3fa9c0`.
pub fn dismiss_finding(
    store: &dyn FindingStore,
    at: DateTime<Utc>,
    id: &str,
    dismissal: Dismissal,
) -> anyhow::Result<Result<FindingId, String>> {
    let mut outcome = Err(String::new());
    store.update(&mut |s| {
        outcome = s
            .dismiss(id, dismissal, at)
            .map(|()| s.get(id).map(|f| f.id.clone()).expect("just dismissed"));
    })?;
    Ok(outcome)
}
