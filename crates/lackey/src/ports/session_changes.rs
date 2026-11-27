//! The session's changes: what the branch changed since it split off the default branch, for
//! triage to read beside a region. Never a fenced file.

use crate::domain::triage_context::SessionDiffs;

pub trait SessionChanges: Send + Sync {
    /// Every file changed since the merge base with the default branch (saved changes and new
    /// files git does not ignore), `file` (relative, `/`) first, the rest in path order, until
    /// the diffs reach the budget of `triage_context::SESSION_CHARS`; none when there is no
    /// default branch to diff against.
    fn diffs(&self, file: &str) -> SessionDiffs;
}
