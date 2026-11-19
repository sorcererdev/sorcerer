//! `scry dismiss`.

use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, Utc};
use sorcerer_core::domain::project::Project;

use crate::adapters::json_finding_store::JsonFindingStore;
use crate::usecases::dismiss_finding::dismiss_finding;

pub(super) fn dismiss(
    project: &Project,
    id: &str,
    everywhere: bool,
    at: DateTime<Utc>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    match dismiss_finding(&JsonFindingStore::for_project(project)?, at, id, everywhere)? {
        Ok(full) => {
            writeln!(
                out,
                "dismissed {} ({})",
                full.0,
                if everywhere { "everywhere" } else { "here" }
            )?;
            Ok(0)
        }
        Err(why) => {
            writeln!(err, "scry: {why}")?;
            Ok(2)
        }
    }
}
