//! What conjure reads about a project beyond the file under the cursor: the files it imports,
//! whether a text parses, scry's findings and the project's rules. Each needs the language
//! grammars or scry's files, which only an adapter may touch.

use std::path::{Path, PathBuf};

use crate::domain::scry_notes::FindingsRead;
use crate::domain::situation::RelatedFile;

/// The files a file imports (with the files beside them), and the imports left out because
/// models may not read them.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RelatedFiles {
    pub files: Vec<RelatedFile>,
    pub left_out: Vec<PathBuf>,
}

pub trait ProjectContext: Send + Sync {
    /// The files `file` imports, each followed by the files beside it that use its names, each
    /// whole with how close it is and the names it defines (at most 8 imports and 6 files
    /// beside them; the step's budget chooses among them). An import a fence refuses is not
    /// read: it is named in `left_out`; a file beside one is skipped.
    /// The text is the file's, which may be the editor's unsaved text.
    fn related_files(&self, file: &Path, text: &str) -> sorcerer_core::Result<RelatedFiles>;

    /// The lines (from 1) where `text` does not parse as the file's language; `None` when no
    /// grammar covers the file, so nothing can be said.
    fn parse_errors(&self, file: &Path, text: &str) -> Option<Vec<u32>>;

    /// scry's findings, as far as conjure can read them.
    fn scry_findings(&self) -> FindingsRead;

    /// The project's rules for the writer, from scry's steering file; empty when it has none.
    fn rules(&self) -> String;
}
