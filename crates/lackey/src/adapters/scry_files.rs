//! scry's findings file, read by lackey's own reader.

use sorcerer_core::Result;
use sorcerer_core::adapters::json_store::Store;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::versioned::Snapshot;

use crate::domain::scry_notes::{FindingsRead, StoredFindings};

/// scry's findings, `.sorcerer/state/scry/findings.json`, read without creating anything. Only
/// version 2 is read; any other is `OtherVersion` with the reason, which `doctor` shows.
pub fn read_findings(project: &Project) -> Result<FindingsRead> {
    let store = Store::<StoredFindings>::at(project.state.join("scry/findings.json"));
    Ok(match store.read()? {
        Snapshot::Absent => FindingsRead::NoFile,
        Snapshot::Current(stored) => FindingsRead::Found(stored.findings),
        Snapshot::Unreadable(why) => FindingsRead::OtherVersion(why),
    })
}
