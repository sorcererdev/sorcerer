//! `scry dismiss`: stop showing a finding, in its unit or everywhere.

use anyhow::Result;
use chrono::{DateTime, Utc};
use sorcerer_core::domain::project::Project;

use super::Console;
use crate::adapters::json_finding_store::JsonFindingStore;
use crate::domain::finding::Dismissal;
use crate::usecases::dismiss_finding::dismiss_finding;

/// Dismisses the finding with this id (or the start of it) and says so,
/// as `dismissed 3fa9c0 (here)`. An id that matches no finding, or more
/// than one, is said on stderr, with exit code 2.
pub(super) fn dismiss(
    project: &Project,
    id: &str,
    dismissal: Dismissal,
    at: DateTime<Utc>,
    console: &mut Console,
) -> Result<i32> {
    match dismiss_finding(&JsonFindingStore::for_project(project)?, at, id, dismissal)? {
        Ok(full) => {
            let reach = match dismissal {
                Dismissal::Here => "here",
                Dismissal::Everywhere => "everywhere",
            };
            writeln!(console.out, "dismissed {} ({reach})", full.0)?;
            Ok(0)
        }
        Err(why) => {
            writeln!(console.err, "scry: {why}")?;
            Ok(2)
        }
    }
}
