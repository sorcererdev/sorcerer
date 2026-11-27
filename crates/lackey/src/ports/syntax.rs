//! What cutting regions needs of a file's grammar: its shape. Loading a grammar is core's
//! adapter work, so the use case asks this port.

use std::path::Path;

use sorcerer_core::domain::language::CommentMarks;

use crate::domain::cutting::Shape;

pub trait Syntax: Send + Sync {
    /// The file's comments, units (stubs marked), prose and headings, and whether a language
    /// read it; a file no language covers, or whose grammar fails, has none of them and is
    /// prose.
    fn shape(&self, file: &Path, text: &str) -> Shape;
    /// How the file's language writes a comment (`#`, `//`, `<!--` … `-->`), for writing an
    /// answer in.
    fn comment_marks(&self, file: &Path) -> Option<CommentMarks>;
}
