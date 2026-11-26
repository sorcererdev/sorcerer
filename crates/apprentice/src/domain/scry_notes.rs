//! What scry knows about the code a task is about: its open findings near the unit, as notes
//! the agent may act on or not. apprentice reads scry's findings file with a reader of its own
//! (a copy of conjure's: products never import each other), so this file holds only the fields
//! apprentice needs.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use sorcerer_core::domain::unit::{Span, Unit};
use sorcerer_core::domain::versioned::Versioned;

/// At most 5 notes go into a situation.
pub const MAX_NOTES: usize = 5;

/// One of scry's findings as apprentice reads it. Fields apprentice does not need are ignored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StoredFinding {
    pub id: String,
    pub issue_type: String,
    #[serde(default)]
    pub rule: Option<String>,
    pub statement: String,
    pub place: StoredPlace,
    #[serde(default)]
    pub evidence: Vec<StoredEvidence>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StoredPlace {
    pub file: PathBuf,
    #[serde(default)]
    pub unit: Option<String>,
    pub lines: Span,
    /// The unit's fingerprint when the finding was placed.
    #[serde(default)]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StoredEvidence {
    pub file: PathBuf,
    pub lines: Span,
}

/// scry's findings file, `.sorcerer/state/scry/findings.json`, as apprentice reads it: version 2.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, serde::Serialize)]
#[serde(default)]
pub struct StoredFindings {
    #[serde(skip_serializing)]
    pub findings: Vec<StoredFinding>,
}

impl Versioned for StoredFindings {
    const VERSION: u32 = 2;
}

/// What reading scry's findings file found.
#[derive(Debug, Clone, PartialEq)]
pub enum FindingsRead {
    Found(Vec<StoredFinding>),
    /// scry has not stored anything here.
    NoFile,
    /// The file is of a version apprentice does not read (or does not read at all); why.
    OtherVersion(String),
}

/// A finding as the agent is told it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScryNote {
    pub issue_type: String,
    pub rule: Option<String>,
    pub statement: String,
    pub file: PathBuf,
    pub line: u32,
    /// The unit changed since scry found this: the note may be out of date.
    pub stale: bool,
}

/// The open findings near a unit, at most 5, those in the unit first.
///
/// A finding counts when its status is `open` or `unverified` (scry's "waiting") and it is
/// placed in the unit (by its name or its lines), in the unit's test file, or cites the unit's
/// lines as evidence. `current_fingerprint(file, lines)` is the fingerprint of the unit that
/// now holds those lines; a note whose stored fingerprint differs is marked `stale`.
///
/// Example: an open finding at `cancel.py:41` while the cursor is in `Cancel.handle` (lines
/// 38-52) is a note; the same finding after `handle` was rewritten is a stale note.
pub fn notes_near(
    findings: &[StoredFinding],
    unit: &Unit,
    file: &Path,
    test_file: Option<&Path>,
    current_fingerprint: impl Fn(&Path, Span) -> Option<String>,
) -> Vec<ScryNote> {
    let near = |finding: &&StoredFinding| {
        matches!(finding.status.as_str(), "open" | "unverified" | "waiting")
            && (in_the_unit(finding, unit, file)
                || in_the_test_file(finding, test_file)
                || cites_the_unit(finding, unit, file))
    };
    let mut notes: Vec<(bool, ScryNote)> = findings
        .iter()
        .filter(near)
        .map(|finding| {
            let stale = is_stale(finding, &current_fingerprint);
            (in_the_unit(finding, unit, file), note_of(finding, stale))
        })
        .collect();
    notes.sort_by_key(|(in_unit, _)| !in_unit);
    notes
        .into_iter()
        .map(|(_, note)| note)
        .take(MAX_NOTES)
        .collect()
}

fn in_the_unit(finding: &StoredFinding, unit: &Unit, file: &Path) -> bool {
    finding.place.file == file
        && (finding.place.unit.as_deref() == Some(unit.name.as_str())
            || unit.span.holds(finding.place.lines.start))
}

fn in_the_test_file(finding: &StoredFinding, test_file: Option<&Path>) -> bool {
    test_file.is_some_and(|tests| finding.place.file == tests)
}

fn cites_the_unit(finding: &StoredFinding, unit: &Unit, file: &Path) -> bool {
    finding.evidence.iter().any(|evidence| {
        evidence.file == file
            && evidence.lines.start <= unit.span.end
            && unit.span.start <= evidence.lines.end
    })
}

/// Whether the unit holding the finding's lines has changed since scry stored its fingerprint.
fn is_stale(
    finding: &StoredFinding,
    current_fingerprint: &impl Fn(&Path, Span) -> Option<String>,
) -> bool {
    match (
        &finding.place.fingerprint,
        current_fingerprint(&finding.place.file, finding.place.lines),
    ) {
        (Some(stored), Some(now)) => *stored != now,
        _ => false,
    }
}

/// The finding as the agent is told it.
pub fn note_of(finding: &StoredFinding, stale: bool) -> ScryNote {
    ScryNote {
        issue_type: finding.issue_type.clone(),
        rule: finding.rule.clone(),
        statement: finding.statement.clone(),
        file: finding.place.file.clone(),
        line: finding.place.lines.start,
        stale,
    }
}
