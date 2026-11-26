//! Where an anchor phrase points in a file. Behind a port so the use cases do not touch a parser.

use std::path::Path;

use sorcerer_core::Result;

use crate::domain::anchor::{Anchor, Resolution};

pub trait Anchors: Send + Sync {
    /// Where the phrase puts text in the file's text. The file (relative to the root) says which
    /// language to read it in; the targets that need no grammar (`marker`, `line matching`,
    /// `file`) work in any file.
    ///
    /// Example: `end of imports` in a file whose last import is on row 3 is new lines at row 4,
    /// not indented.
    fn resolve(&self, file: &Path, text: &str, anchor: &Anchor) -> Result<Resolution>;
}
