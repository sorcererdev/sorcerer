//! What conjure reads about a project beyond the file under the cursor: the files it imports,
//! whether a text parses, scry's findings and the project's rules. Each needs the language
//! grammars or scry's files, which only an adapter may touch.

use std::path::{Path, PathBuf};

use crate::domain::scry_notes::FindingsRead;

pub trait ProjectContext: Send + Sync {
    /// The files `file` imports, with the first lines of each (at most 8 files, 120 lines).
    /// The text is the file's, which may be the editor's unsaved text.
    fn related_files(
        &self,
        file: &Path,
        text: &str,
    ) -> sorcerer_core::Result<Vec<(PathBuf, String)>>;

    /// The lines (from 1) where `text` does not parse as the file's language; `None` when no
    /// grammar covers the file, so nothing can be said.
    fn parse_errors(&self, file: &Path, text: &str) -> Option<Vec<u32>>;

    /// scry's findings, as far as conjure can read them.
    fn scry_findings(&self) -> FindingsRead;

    /// The project's rules for the writer, from scry's steering file; empty when it has none.
    fn rules(&self) -> String;
}
