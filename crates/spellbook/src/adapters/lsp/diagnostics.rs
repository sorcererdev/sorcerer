//! The diagnostics of one file: the marks of the holes still open in it, and the findings of the
//! rules that look at it. They are published together, one list for the file.

use std::path::{Path, PathBuf};

use lsp_types::Diagnostic;

use super::findings::finding_mark;
use super::marks::hole_mark;
use crate::adapters::wiring::Wired;
use crate::usecases::check_files::check_files;
use crate::usecases::open_holes::open_holes;

/// What the editor is told about one file.
pub(super) struct FileDiagnostics {
    pub(super) marks: Vec<Diagnostic>,
    pub(super) findings: Vec<Diagnostic>,
    /// Why the rules could not run (the rules file's problem), when they could not.
    pub(super) rules_problem: Option<String>,
}

impl FileDiagnostics {
    /// The marks then the findings, as one list.
    pub(super) fn all(self) -> Vec<Diagnostic> {
        let mut all = self.marks;
        all.extend(self.findings);
        all
    }
}

/// The marks of the holes open in a file (relative to the root, with `/`), looked for again in
/// the file as it is on disk: a placeholder replaced and saved has no mark. And the findings
/// of the rules over the file as it is on disk (the report keeps them too). Empty when the file
/// cannot be read.
pub(super) fn file_diagnostics(wired: &Wired, file: &Path) -> FileDiagnostics {
    let scope: [PathBuf; 1] = [file.to_path_buf()];
    let marks = open_holes(&wired.holes_deps(), Some(&scope))
        .unwrap_or_default()
        .iter()
        .map(hole_mark)
        .collect();
    let (findings, rules_problem) = match findings_in(wired, &scope) {
        Ok(findings) => (findings, None),
        Err(problem) => (Vec::new(), Some(problem)),
    };
    FileDiagnostics {
        marks,
        findings,
        rules_problem,
    }
}

/// The findings in a file as marks, or the sentence that says why the rules could not run.
fn findings_in(wired: &Wired, files: &[PathBuf]) -> Result<Vec<Diagnostic>, String> {
    let checking = wired.checking()?;
    let found = check_files(&checking.deps(), files).map_err(|e| e.to_string())?;
    Ok(found.iter().map(finding_mark).collect())
}
