//! What spellbook remembers: one record per applied generation (`generations.jsonl`), and the
//! holes still open (`holes.json`).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::versioned::Versioned;

use crate::domain::holes::{Hole, HoleOrigin};
use crate::domain::plan::{FileStatus, Plan};

/// The version written on each line of `generations.jsonl`.
pub const RECORD_VERSION: u32 = 1;

/// One applied generation, as one line of `generations.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationRecord {
    pub version: u32,
    pub id: String,
    pub at: DateTime<Utc>,
    pub generator: String,
    pub inputs: BTreeMap<String, String>,
    pub files: Vec<RecordedFile>,
    pub holes: Vec<Hole>,
    /// The finding a fix answered, when the generation was a fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<RecordedFix>,
    /// The files that were there and were changed, with how many lines were added and removed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<ChangedFile>,
}

/// The finding a fix answered: its rule, where it was, its fingerprint, and which fix (from 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFix {
    pub rule: String,
    pub file: String,
    pub line: u32,
    pub fingerprint: String,
    pub index: usize,
}

/// A file a generation changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    /// The region(s) whose block was rewritten in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// A file a generation wrote or left as it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFile {
    pub path: String,
    pub lines: u32,
    pub status: FileStatus,
}

/// A hole whose placeholder is still in the code, at one place: a row of `holes.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenHole {
    /// The id of the generation that left it.
    pub generation: String,
    pub generator: String,
    pub file: String,
    pub line: u32,
    pub name: String,
    pub kind: String,
    pub instruction: String,
    /// The code that stands in the hole until it is filled: how it is found again.
    pub placeholder: String,
    /// The signs beside the placeholder on its line (see `HolePlace::edges`).
    #[serde(default)]
    pub edges: String,
    /// The pieces of the placeholder's line around the holes on it (see `HolePlace::frame`).
    #[serde(default)]
    pub frame: Vec<String>,
}

/// `holes.json`: the open holes, by file then line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolesFile {
    pub holes: Vec<OpenHole>,
}

impl Versioned for HolesFile {
    const VERSION: u32 = 1;
}

/// The record of a plan applied at a time, keeping only the files that were written (or
/// skipped) and the hole places in them.
///
/// Example: a plan of three files whose second was refused records the first (and any skipped
/// one), and only the holes placed in it.
pub fn record_of(plan: &Plan, at: DateTime<Utc>, kept: &[String]) -> GenerationRecord {
    let files = plan
        .files
        .iter()
        .filter(|file| kept.contains(&file.path))
        .map(|file| RecordedFile {
            path: file.path.clone(),
            lines: file.lines,
            status: file.status,
        })
        .collect();
    GenerationRecord {
        version: RECORD_VERSION,
        id: plan.id.clone(),
        at,
        generator: plan.generator.clone(),
        inputs: plan.inputs.clone(),
        files,
        holes: holes_in(&plan.holes, kept),
        fix: plan.fix.as_ref().map(|fix| RecordedFix {
            rule: fix.rule.clone(),
            file: fix.file.clone(),
            line: fix.line,
            fingerprint: fix.fingerprint.clone(),
            index: fix.index,
        }),
        changed: plan
            .files
            .iter()
            .filter(|file| file.status == FileStatus::Changed && kept.contains(&file.path))
            .map(|file| ChangedFile {
                path: file.path.clone(),
                added: file.added,
                removed: file.removed,
                region: file.region.clone(),
            })
            .collect(),
    }
}

/// The open hole places a plan leaves in the files written: one row per placeholder place.
pub fn open_holes_of(plan: &Plan, written: &[String]) -> Vec<OpenHole> {
    holes_in(&plan.holes, written)
        .into_iter()
        .filter(|hole| hole.source == HoleOrigin::Placeholder)
        .flat_map(|hole| {
            hole.places
                .iter()
                .map(move |place| OpenHole {
                    generation: plan.id.clone(),
                    generator: plan.generator.clone(),
                    file: place.file.clone(),
                    line: place.line,
                    name: hole.name.clone(),
                    kind: hole.kind.clone(),
                    instruction: hole.instruction.clone(),
                    placeholder: place.text.clone(),
                    edges: place.edges.clone(),
                    frame: place.frame.clone(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The holes with only their places in the given files; a hole left with no place is dropped.
fn holes_in(holes: &[Hole], files: &[String]) -> Vec<Hole> {
    holes
        .iter()
        .map(|hole| Hole {
            places: hole
                .places
                .iter()
                .filter(|place| files.contains(&place.file))
                .cloned()
                .collect(),
            ..hole.clone()
        })
        .filter(|hole| !hole.places.is_empty())
        .collect()
}

/// Open holes in the order `spellbook holes` lists them: by file, then line, then name.
pub fn sorted_open_holes(mut holes: Vec<OpenHole>) -> Vec<OpenHole> {
    holes.sort_by(|a, b| (&a.file, a.line, &a.name).cmp(&(&b.file, b.line, &b.name)));
    holes
}
