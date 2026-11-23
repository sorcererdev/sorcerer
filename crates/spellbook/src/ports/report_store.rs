//! The last check's report, kept between runs.

use chrono::{DateTime, Utc};
use sorcerer_core::Result;

use crate::domain::finding::Finding;
use crate::domain::report::ReportFile;

pub trait ReportStore: Send + Sync {
    /// The saved report; empty when nothing was saved yet.
    fn read(&self) -> Result<ReportFile>;
    /// Replaces the findings of these files (and no others), and the time.
    fn replace_files(&self, files: &[String], found: Vec<Finding>, at: DateTime<Utc>)
    -> Result<()>;
    /// Replaces the whole report.
    fn replace_all(&self, found: Vec<Finding>, at: DateTime<Utc>) -> Result<()>;
}
