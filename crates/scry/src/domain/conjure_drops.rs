//! What conjure left for scry's next review: code a rules retry dropped that Jev did not call
//! legitimate. conjure writes `.sorcerer/state/conjure/drops.json`; scry reads it with a reader
//! of its own (products never import each other), so only the fields scry needs are here.
//!
//! A drop goes into the review of its file while the file still lacks the dropped name, so the
//! reviewer can raise a finding when behaviour was lost.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sorcerer_core::domain::versioned::Versioned;

/// One drop as scry reads it.
///
/// Example: `{ name: "std::fs::write", kind: "call", file: "src/seat.rs", line: 36, verdict:
/// "looks unintended", rules: ["commands-use-ports: Commands talk to ports"] }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConjureDrop {
    pub name: String,
    /// `call` or `definition`.
    pub kind: String,
    pub file: PathBuf,
    pub line: u32,
    /// Jev's verdict in words: `looks unintended`, `maybe unintended`, `not checked`.
    pub verdict: String,
    #[serde(default)]
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct StoredNote {
    drop: ConjureDrop,
}

/// conjure's drop notes file as scry reads it: version 1.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct ConjureDropNotes {
    #[serde(skip_serializing)]
    notes: Vec<StoredNote>,
}

impl Versioned for ConjureDropNotes {
    const VERSION: u32 = 1;
}

impl ConjureDropNotes {
    pub fn drops(self) -> Vec<ConjureDrop> {
        self.notes.into_iter().map(|note| note.drop).collect()
    }
}

/// Whether the file's text still lacks the dropped name, so the drop still stands.
///
/// Example: a drop of `std::fs::write` stands in a file whose text has no `std::fs::write`.
pub fn still_dropped(drop: &ConjureDrop, text: &str) -> bool {
    !text.contains(&drop.name)
}

/// The prompt's part about the drops, empty when there are none: each drop with its file, the
/// rule conjure was asked to keep and Jev's verdict.
///
/// Example: ``- the call `std::fs::write`, at src/seat.rs:36 before the retry (rule
/// commands-use-ports: Commands talk to ports; Jev: looks unintended)``.
pub fn drops_section(drops: &[ConjureDrop]) -> String {
    if drops.is_empty() {
        return String::new();
    }
    let rows: String = drops.iter().map(drop_row).collect();
    format!(
        "conjure wrote part of this change. Its first answer broke a project rule, so it was asked again, and the second answer dropped code the first had:\n{rows}Check whether the change still does what that code did. When behaviour the change needs was lost, report it as a bug at the place it went missing.\n\n"
    )
}

fn drop_row(drop: &ConjureDrop) -> String {
    let rules = if drop.rules.is_empty() {
        String::new()
    } else {
        format!("rule {}; ", drop.rules.join(", "))
    };
    format!(
        "- the {} `{}`, at {}:{} before the retry ({rules}Jev: {})\n",
        drop.kind,
        drop.name,
        sorcerer_core::domain::paths::forward_slashes(&drop.file),
        drop.line,
        drop.verdict
    )
}
