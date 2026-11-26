//! A plan: every file a generation would write, with its text, and every hole, decided before
//! anything is written. Also the rules a planned path keeps, and why a plan stops.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sorcerer_core::domain::fnv::fnv64_hex;

use crate::domain::change::{Hunk, TextChange, added_and_removed, changes_between, hunks};
use crate::domain::holes::{Hole, HoleOrigin};
use crate::domain::inputs::{InputStop, input_stop_text};

/// What a generation would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// Eight hex digits naming this plan (and the generation, once applied).
    pub id: String,
    pub generator: String,
    pub inputs: BTreeMap<String, String>,
    pub files: Vec<PlannedFile>,
    /// Every hole, derived ones included, in the order they first appear.
    pub holes: Vec<Hole>,
    /// The finding this plan fixes, when it is a fix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixOrigin>,
}

/// The finding a fix answers, and which of its rule's fixes it is (from 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixOrigin {
    pub rule: String,
    pub file: String,
    pub line: u32,
    pub fingerprint: String,
    pub index: usize,
    pub label: String,
}

impl Plan {
    /// The holes someone has to fill: those a placeholder stands in.
    pub fn placeholder_holes(&self) -> Vec<&Hole> {
        self.holes
            .iter()
            .filter(|hole| hole.source == HoleOrigin::Placeholder)
            .collect()
    }

    /// The holes the inputs filled.
    pub fn derived_holes(&self) -> Vec<&Hole> {
        self.holes
            .iter()
            .filter(|hole| hole.source == HoleOrigin::Derived)
            .collect()
    }

    /// The files the plan writes.
    pub fn new_files(&self) -> Vec<&PlannedFile> {
        self.files
            .iter()
            .filter(|file| file.status == FileStatus::New)
            .collect()
    }

    /// The files the plan changes.
    pub fn changed_files(&self) -> Vec<&PlannedFile> {
        self.files
            .iter()
            .filter(|file| file.status == FileStatus::Changed)
            .collect()
    }
}

/// One file of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedFile {
    /// Relative to the root, with `/`.
    pub path: String,
    pub status: FileStatus,
    /// The text to write (for a changed file, its text after the plan); empty for a skipped file.
    pub text: String,
    pub lines: u32,
    /// For a changed file: the changes against `before`, whole lines, and the diff to show.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<TextChange>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hunks: Vec<Hunk>,
    /// For a changed file: its text when the plan was made, which it must still have when the
    /// plan is applied.
    #[serde(skip)]
    pub before: String,
    /// For a changed file: how many lines the plan adds and removes.
    #[serde(skip_serializing_if = "is_zero")]
    pub added: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub removed: u32,
}

fn is_zero(count: &u32) -> bool {
    *count == 0
}

impl PlannedFile {
    /// A file the plan writes new.
    pub fn new_file(path: String, text: String) -> PlannedFile {
        PlannedFile {
            lines: line_count(&text),
            path,
            status: FileStatus::New,
            text,
            changes: Vec::new(),
            hunks: Vec::new(),
            before: String::new(),
            added: 0,
            removed: 0,
        }
    }

    /// A file that is there, changed from `before` to `after`: its changes and diff worked out.
    /// The same text both times is a file left unchanged.
    pub fn changed(path: String, before: String, after: String) -> PlannedFile {
        if before == after {
            return PlannedFile::left(path, FileStatus::Unchanged);
        }
        let hunks = hunks(&before, &after);
        let (added, removed) = added_and_removed(&hunks);
        PlannedFile {
            lines: line_count(&after),
            changes: changes_between(&before, &after),
            hunks,
            added: u32::try_from(added).unwrap_or(u32::MAX),
            removed: u32::try_from(removed).unwrap_or(u32::MAX),
            path,
            status: FileStatus::Changed,
            text: after,
            before,
        }
    }

    /// Whether the plan leaves this file as it is: skipped, or every edit to it in place already.
    pub fn leaves_the_code_as_it_is(&self) -> bool {
        matches!(self.status, FileStatus::Skipped | FileStatus::Unchanged)
    }

    /// A file the plan leaves as it is: skipped (there already) or unchanged (every insert's
    /// `unless` holds).
    pub fn left(path: String, status: FileStatus) -> PlannedFile {
        PlannedFile {
            path,
            status,
            text: String::new(),
            lines: 0,
            changes: Vec::new(),
            hunks: Vec::new(),
            before: String::new(),
            added: 0,
            removed: 0,
        }
    }
}

/// Whether a planned file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    New,
    /// It is there already and the edit says `on_exists = "skip"`.
    Skipped,
    /// It is there and the plan changes some of its lines.
    Changed,
    /// It is there and every edit to it is in place already (its `unless` holds).
    Unchanged,
}

/// What a plan with nothing to do was: a fix, or a generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    Fix,
    Generation,
}

/// Why a plan stopped before anything was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanStop {
    Input(InputStop),
    /// No generator by that name, or one that did not load.
    Generator(String),
    PathRefused {
        path: String,
        why: String,
    },
    Exists {
        path: String,
    },
    DoesNotParse {
        template: String,
        file: String,
        line: u32,
    },
    NoLanguageForHole {
        path: String,
        hole: String,
    },
    Template(String),
    /// The finding a fix answers is no longer on that line.
    FindingGone {
        file: String,
        line: u32,
        rule: String,
    },
    /// The fix's `replace` names no node kind around the finding.
    NoReplaceCase {
        kind: String,
    },
    /// An insert's file is not there, or none of its phrases resolves.
    Anchor {
        edit: String,
        file: String,
        why: String,
    },
    /// Every edit is in place already and nothing is new.
    NothingToDo(Applied),
}

/// The stop as one sentence.
///
/// Example: `src/x/cancel_table.py is there already (on_exists = "skip" would leave it)`.
pub fn plan_stop_text(stop: &PlanStop) -> String {
    match stop {
        PlanStop::Input(stop) => input_stop_text(stop),
        PlanStop::Generator(why) | PlanStop::Template(why) => why.clone(),
        PlanStop::PathRefused { path, why } => format!("{path}: {why}; nothing was written"),
        PlanStop::Exists { path } => format!(
            "{path} is there already; nothing was written (on_exists = \"skip\" would leave it)"
        ),
        PlanStop::DoesNotParse {
            template,
            file,
            line,
        } => {
            format!("{file} would not parse at line {line} (from {template}); nothing was written")
        }
        PlanStop::NoLanguageForHole { path, hole } => {
            format!("no language covers {path}, so its hole {hole} has no placeholder")
        }
        PlanStop::FindingGone { file, line, rule } => format!(
            "{file}:{line} has no {rule} finding any more; nothing to fix (run `spellbook check`)"
        ),
        PlanStop::NoReplaceCase { kind } => format!(
            "the finding is a `{kind}`, and no `replace` case of the fix names a node around it; nothing was written"
        ),
        PlanStop::Anchor { edit, file, why } => {
            format!("{edit} ({file}): {why}; nothing was written")
        }
        PlanStop::NothingToDo(Applied::Fix) => "nothing to do: the fix is already applied".into(),
        PlanStop::NothingToDo(Applied::Generation) => {
            "nothing to do: every edit is in the code already".into()
        }
    }
}

/// A path as a generator wrote it, kept only when it stays inside the project: not absolute,
/// no `..` climbing out, `.` and empty parts dropped. The result is written with `/`.
///
/// Example: `src/./a//b.x` is `src/a/b.x`; `../x` and `/x` and `C:\x` are refused.
pub fn relative_path(text: &str) -> Result<String, String> {
    let has_drive = text.len() >= 2 && text.as_bytes()[1] == b':';
    if text.starts_with(['/', '\\']) || has_drive {
        return Err("an absolute path; a generator writes paths inside the project".into());
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in text.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts
                    .pop()
                    .ok_or_else(|| "the path leaves the project".to_string())?;
            }
            name => parts.push(name),
        }
    }
    if parts.is_empty() {
        return Err("the path names no file".into());
    }
    Ok(parts.join("/"))
}

/// A plan's id: the first eight hex digits of a hash of the generator, its inputs and the time.
pub fn plan_id(generator: &str, inputs: &BTreeMap<String, String>, at: &str) -> String {
    let inputs: Vec<String> = inputs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let key = format!("{generator}\n{}\n{at}", inputs.join("\n"));
    fnv64_hex(key.as_bytes())[..8].to_string()
}

/// How many lines a text has.
pub fn line_count(text: &str) -> u32 {
    u32::try_from(text.lines().count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_in_the_project() {
        assert_eq!(relative_path("src/./a//b.x"), Ok("src/a/b.x".into()));
        assert_eq!(relative_path("src/a/../b.x"), Ok("src/b.x".into()));
        for outside in ["../x", "/abs/x", "C:\\x", "\\x", "a/../../x", ""] {
            assert!(relative_path(outside).is_err(), "{outside}");
        }
    }

    #[test]
    fn the_id_is_eight_hex_digits_and_depends_on_the_time() {
        let inputs = BTreeMap::from([("a".to_string(), "b".to_string())]);
        let one = plan_id("g", &inputs, "t1");
        assert_eq!(one.len(), 8);
        assert_ne!(one, plan_id("g", &inputs, "t2"));
    }
}
