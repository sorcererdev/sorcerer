//! `scry dismiss`.

use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, Utc};
use sorcerer_core::domain::project::Project;

use crate::adapters::json_finding_store::JsonFindingStore;
use crate::domain::finding::Dismissal;
use crate::usecases::dismiss_finding::dismiss_finding;

pub(super) fn dismiss(
    project: &Project,
    id: &str,
    dismissal: Dismissal,
    at: DateTime<Utc>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    match dismiss_finding(&JsonFindingStore::for_project(project)?, at, id, dismissal)? {
        Ok(full) => {
            let reach = match dismissal {
                Dismissal::Here => "here",
                Dismissal::Everywhere => "everywhere",
            };
            writeln!(out, "dismissed {} ({reach})", full.0)?;
            Ok(0)
        }
        Err(why) => {
            writeln!(err, "scry: {why}")?;
            Ok(2)
        }
    }
}
