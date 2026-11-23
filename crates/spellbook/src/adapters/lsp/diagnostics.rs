//! The diagnostics of one file: the marks of the holes still open in it.

use std::path::{Path, PathBuf};

use lsp_types::Diagnostic;

use super::marks::hole_mark;
use crate::adapters::wiring::Wired;
use crate::usecases::open_holes::open_holes;

/// The marks of the holes open in a file (relative to the root, with `/`), looked for again in
/// the file as it is on disk: a placeholder replaced and saved has no mark. Empty when the file
/// cannot be read.
pub(super) fn file_diagnostics(wired: &Wired, file: &Path) -> Vec<Diagnostic> {
    let scope: [PathBuf; 1] = [file.to_path_buf()];
    open_holes(&wired.holes_deps(), Some(&scope))
        .unwrap_or_default()
        .iter()
        .map(hole_mark)
        .collect()
}
