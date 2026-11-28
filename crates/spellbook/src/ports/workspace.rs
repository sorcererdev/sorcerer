//! Where a generation's files are written.

use std::path::Path;

/// Why a file was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteRefused {
    /// A file is there already (it may have appeared after the plan).
    Exists,
    /// The path leads out of the project or onto a fence, once its links are followed.
    Fenced,
    /// The file is not what the plan saw: it was edited (or removed) after the plan.
    Changed,
    /// The disk said no.
    Disk(String),
}

/// The refusal as words.
///
/// Example: `it is there already (it appeared after the plan)`.
pub fn write_refused_text(refused: &WriteRefused) -> String {
    match refused {
        WriteRefused::Exists => "it is there already (it appeared after the plan)".into(),
        WriteRefused::Fenced => {
            "it leads out of the project or onto a fenced path once its links are followed".into()
        }
        WriteRefused::Changed => {
            "it changed since the plan; nothing was applied: plan it again".into()
        }
        WriteRefused::Disk(why) => why.clone(),
    }
}

pub trait Workspace: Send + Sync {
    /// Writes a new file's bytes (its text, or a copied binary file) at a path relative to the
    /// root, its folders created; never over a file that is there.
    fn write_new(&self, path: &Path, bytes: &[u8]) -> Result<(), WriteRefused>;

    /// Replaces a file's text with `after`, only when the file still holds `before` (the text a
    /// plan was made from).
    fn write_changed(&self, path: &Path, before: &str, after: &str) -> Result<(), WriteRefused>;
}
