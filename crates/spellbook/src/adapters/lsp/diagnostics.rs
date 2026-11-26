//! The diagnostics of one file: the marks of the holes still open in it, and the findings of the
//! rules that look at it. They are published together, one list for the file.

use std::path::{Path, PathBuf};

use lsp_types::Diagnostic;
use serde_json::{Value, json};

use super::findings::finding_mark;
use super::marks::hole_mark;
use crate::adapters::wiring::Wired;
use crate::domain::finding::Finding;
use crate::domain::record::OpenHole;
use crate::usecases::check_files::check_files;
use crate::usecases::open_holes::open_holes;
use sorcerer_core::ports::view::CodebaseView;

/// What the editor is told about one file.
pub(super) struct FileDiagnostics {
    pub(super) marks: Vec<Diagnostic>,
    /// The open holes as a session's tab stops, in the order a person walks them (line, then
    /// column); see [`hole_stops`].
    pub(super) stops: Vec<Value>,
    /// The rules' findings in the file, as the code actions read them back.
    pub(super) findings: Vec<Finding>,
    /// Why the rules could not run (the rules file's problem), when they could not.
    pub(super) rules_problem: Option<String>,
}

impl FileDiagnostics {
    /// The marks then the findings, as one list.
    pub(super) fn all(self) -> Vec<Diagnostic> {
        let mut all = self.marks;
        all.extend(self.findings.iter().map(finding_mark));
        all
    }
}

/// The marks of the holes open in a file (relative to the root, with `/`), looked for again in
/// the file as it is on disk: a placeholder replaced and saved has no mark. And the findings
/// of the rules over the file as it is on disk (the report keeps them too). Empty when the file
/// cannot be read.
pub(super) fn file_diagnostics(wired: &Wired, file: &Path) -> FileDiagnostics {
    let scope: [PathBuf; 1] = [file.to_path_buf()];
    let holes = open_holes(&wired.holes_deps(), Some(&scope)).unwrap_or_default();
    let text = wired.view.text(file).unwrap_or_default();
    let rows: Vec<&str> = text.lines().collect();
    let row_of = |hole: &OpenHole| rows.get(hole.line.saturating_sub(1) as usize).copied();
    let marks: Vec<Diagnostic> = holes
        .iter()
        .map(|hole| hole_mark(hole, row_of(hole)))
        .collect();
    let stops = hole_stops(&holes, &marks);
    let (findings, rules_problem) = match findings_in(wired, &scope) {
        Ok(findings) => (findings, None),
        Err(problem) => (Vec::new(), Some(problem)),
    };
    FileDiagnostics {
        marks,
        stops,
        findings,
        rules_problem,
    }
}

/// The findings in a file, or the sentence that says why the rules could not run.
fn findings_in(wired: &Wired, files: &[PathBuf]) -> Result<Vec<Finding>, String> {
    let checking = wired.checking()?;
    check_files(&checking.deps(), files).map_err(|e| e.to_string())
}

/// The holes as tab stops: each with its file, name, kind, instruction and the range of its
/// placeholder (protocol positions, 0-based, UTF-16), sorted by line then column.
///
/// Example: `field` at 4:4..4:14 comes before `type` at 4:16..4:18 on the same line.
fn hole_stops(holes: &[OpenHole], marks: &[Diagnostic]) -> Vec<Value> {
    let mut stops: Vec<(&Diagnostic, &OpenHole)> = marks.iter().zip(holes).collect();
    stops.sort_by_key(|(mark, _)| (mark.range.start.line, mark.range.start.character));
    stops
        .into_iter()
        .map(|(mark, hole)| {
            json!({
                "file": hole.file,
                "name": hole.name,
                "kind": hole.kind,
                "instruction": hole.instruction,
                "range": mark.range,
            })
        })
        .collect()
}
