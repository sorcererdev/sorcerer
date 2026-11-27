//! Checking the whole project: every file a rule or an allow comment may be in.

use std::path::PathBuf;

use sorcerer_core::Result;

use crate::domain::finding::Finding;
use crate::usecases::check_files::{CheckDeps, findings_of_files};

/// Checks every file the project has and rewrites the report whole. Returns the findings, by
/// file then line.
///
/// Example: a project with one command importing `sqlite3` returns that one finding; the report
/// then holds just it.
pub fn check_project(deps: &CheckDeps) -> Result<Vec<Finding>> {
    let files: Vec<PathBuf> = deps
        .view
        .glob("**")?
        .into_iter()
        .filter(|file| !is_spellbooks_own(file))
        .collect();
    let found = findings_of_files(deps, &files)?;
    deps.report.replace_all(found.clone(), deps.clock.now())?;
    Ok(found)
}

/// The project's own settings and state are never checked.
pub(super) fn is_spellbooks_own(file: &std::path::Path) -> bool {
    file.starts_with(".sorcerer") || file.starts_with(".git")
}
