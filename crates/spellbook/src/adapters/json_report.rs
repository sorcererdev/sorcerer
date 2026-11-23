//! The report as core's store file, `.sorcerer/state/spellbook/report.json`
//! (`{"version": 1, "data": {"checked": …, "findings": […]}}`).

use chrono::{DateTime, Utc};
use sorcerer_core::adapters::json_store::{Snapshot, Store};
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::finding::Finding;
use crate::domain::report::ReportFile;
use crate::ports::report_store::ReportStore;

/// Where the report is kept, under the project's state directory.
pub const REPORT_FILE: &str = "spellbook/report.json";

pub struct JsonReport {
    pub store: Store<ReportFile>,
}

impl JsonReport {
    pub fn for_project(project: &Project) -> Result<JsonReport> {
        Ok(JsonReport {
            store: Store::in_project(project, REPORT_FILE)?,
        })
    }
}

impl ReportStore for JsonReport {
    /// An unreadable file is an error and is left alone.
    fn read(&self) -> Result<ReportFile> {
        match self.store.read()? {
            Snapshot::Absent => Ok(ReportFile::default()),
            Snapshot::Current(file) => Ok(file),
            Snapshot::Unreadable(why) => Err(Error::StoreUnreadable {
                path: self.store.path().to_path_buf(),
                why,
            }),
        }
    }

    fn replace_files(
        &self,
        files: &[String],
        found: Vec<Finding>,
        at: DateTime<Utc>,
    ) -> Result<()> {
        self.store.update(|report| {
            *report = std::mem::take(report).with_files_replaced(files, found, at);
        })
    }

    fn replace_all(&self, mut found: Vec<Finding>, at: DateTime<Utc>) -> Result<()> {
        crate::domain::report::sort_findings(&mut found);
        self.store.update(|report| {
            *report = ReportFile {
                checked: Some(at),
                findings: found,
            };
        })
    }
}
