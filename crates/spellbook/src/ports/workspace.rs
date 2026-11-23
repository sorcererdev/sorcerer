//! Where a generation's files are written.

use std::path::Path;

/// Why a file was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteRefused {
    /// A file is there already (it may have appeared after the plan).
    Exists,
    /// The path leads out of the project or onto a fence, once its links are followed.
    Fenced,
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
        WriteRefused::Disk(why) => why.clone(),
    }
}

pub trait Workspace: Send + Sync {
    /// Writes a new file at a path relative to the root, its folders created; never over a file
    /// that is there.
    fn write_new(&self, path: &Path, text: &str) -> Result<(), WriteRefused>;
}
