//! What conjure leaves for scry's next review: the drops of a rules retry that Jev did not
//! call legitimate, in `.sorcerer/state/conjure/drops.json`. scry reads the file with a reader
//! of its own (products never import each other) and tells its reviewer about a drop when it
//! reviews that file.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::versioned::Versioned;

use crate::domain::record::RecordedDrop;
use crate::domain::retry_drops::CheckedDrop;

/// The newest 20 notes are kept; an older one is let go when a new one comes.
pub const MAX_NOTES: usize = 20;

/// One drop left for scry, with the change it came from.
///
/// Example: `{ "change": "3fa9c0", "at": "2026-11-27T12:00:00Z", "drop": { "name":
/// "std::fs::write", "kind": "call", "file": "src/seat.rs", "line": 36, "verdict": "looks
/// unintended", "rules": ["commands-use-ports: Commands talk to ports"] } }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropNote {
    pub change: String,
    pub at: DateTime<Utc>,
    pub drop: RecordedDrop,
}

/// The notes file, version 1.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropNotes {
    pub notes: Vec<DropNote>,
}

impl Versioned for DropNotes {
    const VERSION: u32 = 1;
}

impl DropNotes {
    /// The notes with a change's said drops added (a legitimate drop is not left for scry), the
    /// newest 20 kept.
    pub fn with_change(&self, change: &str, at: DateTime<Utc>, drops: &[CheckedDrop]) -> DropNotes {
        let added = drops
            .iter()
            .filter(|drop| drop.verdict.is_said())
            .map(|drop| DropNote {
                change: change.to_string(),
                at,
                drop: RecordedDrop::from(drop),
            });
        let all: Vec<DropNote> = self.notes.iter().cloned().chain(added).collect();
        let skip = all.len().saturating_sub(MAX_NOTES);
        DropNotes {
            notes: all.into_iter().skip(skip).collect(),
        }
    }

    /// The notes without a change's: it was undone or replaced, so its drops are gone too.
    pub fn without_change(&self, change: &str) -> DropNotes {
        DropNotes {
            notes: self
                .notes
                .iter()
                .filter(|note| note.change != change)
                .cloned()
                .collect(),
        }
    }
}
