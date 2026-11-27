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

    /// Where the phrase puts `inserting` in the file's text: as [`Anchors::resolve`], except that
    /// it may choose among the target's places by the text itself. `end of imports` keeps imports
    /// that are already in order in order.
    ///
    /// Example: `end of imports` with `import b` in a file whose last imports are `import a` and
    /// `import c` (in order) is the row of `import c`, not the row after it.
    fn resolve_inserting(
        &self,
        file: &Path,
        text: &str,
        anchor: &Anchor,
        inserting: &str,
    ) -> Result<Resolution> {
        let _ = inserting;
        self.resolve(file, text, anchor)
    }
}
