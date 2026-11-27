//! Checking texts that are not on disk yet: what another tool means to write (conjure's change
//! before it lands), each text checked as the file at its path would be.

use std::collections::BTreeMap;
use std::path::PathBuf;

use sorcerer_core::Result;

use crate::domain::finding::Finding;
use crate::domain::report::sort_findings;
use crate::usecases::check_files::{CheckDeps, rule_findings_of_files};
use crate::usecases::planned_view::PlannedView;

/// The rules' findings in these texts, each read as the file at its path (relative to the root,
/// with `/`), by file then line. Regions are not checked (a new file leaves a list for `sync`,
/// which is not a broken rule), and nothing is saved: the report keeps what is on disk. The
/// baseline still marks what is existing.
///
/// Example: `{"src/bistro/booking/commands/close_table.py": "import sqlite3\n"}` gives one
/// `commands-use-ports` finding at line 1, whatever the file on disk holds.
pub fn check_texts(deps: &CheckDeps, texts: BTreeMap<String, String>) -> Result<Vec<Finding>> {
    let files: Vec<PathBuf> = texts.keys().map(PathBuf::from).collect();
    let view = PlannedView::new(deps.view, texts);
    let over_the_texts = CheckDeps {
        view: &view,
        ..*deps
    };
    let mut found = rule_findings_of_files(&over_the_texts, &files)?;
    sort_findings(&mut found);
    Ok(deps.baseline.marked(found))
}
