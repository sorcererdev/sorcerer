//! The marks a landed change shows: one Information diagnostic on the first changed line of
//! each file, from source `conjure`, carrying the change's id.

use std::path::PathBuf;

/// One mark: which change, in which file (as the editor names it), on which line (from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Mark {
    pub change_id: String,
    pub path: PathBuf,
    pub line: u32,
    pub kind: MarkKind,
}

/// What a mark says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MarkKind {
    /// On the file the change was asked in: the goal, the model, the cost, the time, whether
    /// it parses, the other files.
    Main,
    /// On each other changed file: which file the change was asked in, and its goal.
    Other,
    /// On the unit's first line when the writer changed nothing.
    NoChange,
}
