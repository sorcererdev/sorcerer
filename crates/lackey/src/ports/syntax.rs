//! What cutting regions needs of a file's grammar: its shape. Loading a grammar is core's
//! adapter work, so the use case asks this port.

use std::path::Path;

use crate::domain::cutting::Shape;

pub trait Syntax: Send + Sync {
    /// The file's comments, units (stubs marked) and whether a language covers it; a file no
    /// language covers, or whose grammar fails, has no comments or units.
    fn shape(&self, file: &Path, text: &str) -> Shape;
    /// How a line comment starts in the file's language (`#`, `//`), for writing an answer in.
    fn comment_prefix(&self, file: &Path) -> Option<String>;
}
