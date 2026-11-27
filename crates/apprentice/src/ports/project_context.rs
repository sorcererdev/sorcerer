//! What a task's first message says about the project around the place: the unit, scry's open
//! findings near it, the project's rules. Each needs grammars or scry's files, so the use case
//! asks this port.

use std::path::Path;

use crate::domain::scry_notes::ScryNote;

pub trait ProjectContext: Send + Sync {
    /// The unit holding the line (from 1) of the text: its name and its text.
    fn unit_at(&self, file: &Path, text: &str, line: u32) -> Option<(String, String)>;
    /// scry's open findings near the line.
    fn notes_near(&self, file: &Path, text: &str, line: u32) -> Vec<ScryNote>;
    /// The project's rules, from scry's steering file; empty when there are none.
    fn rules(&self) -> String;
}
