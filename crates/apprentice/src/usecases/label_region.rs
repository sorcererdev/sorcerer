//! The person's word on a region: a label (what it really was) or a dismissal. Both are lines
//! of the corpus (`labels.jsonl`); a dismissal also takes the region's mark away for good.

use sorcerer_core::Result;

use crate::domain::region_record::{Label, RegionRecord};
use crate::domain::triage::Kind;
use crate::ports::clock::Clock;
use crate::ports::region_store::RegionStore;

/// Records what the person says the region was (`None`: nothing).
pub fn label_region(
    record: &RegionRecord,
    person: Option<Kind>,
    regions: &dyn RegionStore,
    clock: &dyn Clock,
) -> Result<()> {
    regions.label(&label_of(record, person, "label", clock))
}

/// Dismisses the region: never shown again at this text, and a line of the corpus.
pub fn dismiss_region(
    record: &RegionRecord,
    regions: &dyn RegionStore,
    clock: &dyn Clock,
) -> Result<()> {
    let dismissed = RegionRecord {
        dismissed: true,
        ..record.clone()
    };
    regions.put(std::slice::from_ref(&dismissed))?;
    regions.label(&label_of(record, None, "dismiss", clock))
}

fn label_of(record: &RegionRecord, person: Option<Kind>, how: &str, clock: &dyn Clock) -> Label {
    Label {
        region: record.region.clone(),
        jev: record.triaged.jev_kind,
        acted_as: record.triaged.kind,
        person,
        how: how.into(),
        at: clock.now(),
    }
}
