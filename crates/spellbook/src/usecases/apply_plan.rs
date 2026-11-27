//! A plan applied: its changed files landed, its new files written, the generation recorded,
//! the open holes kept.

use std::path::Path;

use crate::domain::plan::{FileStatus, Plan};
use crate::domain::record::{OpenHole, open_holes_of, record_of, sorted_open_holes};
use crate::ports::clock::Clock;
use crate::ports::generation_store::GenerationStore;
use crate::ports::holes_store::HolesStore;
use crate::ports::workspace::{Workspace, WriteRefused};

/// What applying writes to.
pub struct ApplyDeps<'a> {
    pub workspace: &'a dyn Workspace,
    pub generations: &'a dyn GenerationStore,
    pub holes: &'a dyn HolesStore,
    pub clock: &'a dyn Clock,
}

/// What applying did: the files changed, written and skipped, and the file refused, if one was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedGeneration {
    pub plan: Plan,
    pub changed: Vec<String>,
    pub written: Vec<String>,
    pub skipped: Vec<String>,
    /// The file that stopped the writing, and why; the files after it were not written.
    pub refused: Option<(String, WriteRefused)>,
}

/// Who lands a plan's changed files: this use case, through the workspace (the command line),
/// or the editor, which applied them as one undoable edit before asking for the rest (the
/// server).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangesLanded {
    NotYet,
    ByTheEditor,
}

/// Applies a plan: (1) lands its changed files, each only if the disk still has the text the
/// plan saw, stopping at the first refusal with nothing else written; (2) writes its new files in
/// order, stopping at the first refusal (the files already written stay: each is a whole new file
/// the person can delete); (3) appends the generation's record; (4) adds the placeholder places in
/// the changed and written files to the open holes.
///
/// Example: if `cancel_table.py` appeared between the plan and now, it is refused, the test
/// file after it is not written, and the record names only what was; if `close_table.py` was
/// edited after a fix was planned, nothing is written at all.
pub fn apply_plan(
    deps: &ApplyDeps,
    plan: Plan,
    landed: ChangesLanded,
) -> sorcerer_core::Result<AppliedGeneration> {
    let changed = match land_changed_files(deps.workspace, &plan, landed) {
        Ok(changed) => changed,
        Err(refused) => {
            return Ok(AppliedGeneration {
                plan,
                changed: Vec::new(),
                written: Vec::new(),
                skipped: Vec::new(),
                refused: Some(refused),
            });
        }
    };
    let (written, refused) = write_new_files(deps.workspace, &plan);
    let skipped: Vec<String> = plan
        .files
        .iter()
        .filter(|file| file.status == FileStatus::Skipped)
        .map(|file| file.path.clone())
        .collect();
    let kept: Vec<String> = written
        .iter()
        .chain(&skipped)
        .chain(&changed)
        .cloned()
        .collect();
    deps.generations
        .append(&record_of(&plan, deps.clock.now(), &kept))?;
    let with_holes: Vec<String> = changed.iter().chain(&written).cloned().collect();
    let open = remembered_outside(deps.holes.read()?, &written)
        .into_iter()
        .chain(open_holes_of(&plan, &with_holes))
        .collect();
    deps.holes.replace(sorted_open_holes(open))?;
    Ok(AppliedGeneration {
        plan,
        changed,
        written,
        skipped,
        refused,
    })
}

/// The remembered holes that are not in a file just written whole. A new file replaces whatever
/// was remembered of a file at its path: a file deleted by hand keeps its rows (a stash may bring
/// it back), so a second generation writing it again would otherwise list each hole twice.
fn remembered_outside(saved: Vec<OpenHole>, written: &[String]) -> Vec<OpenHole> {
    saved
        .into_iter()
        .filter(|hole| !written.contains(&hole.file))
        .collect()
}

/// The plan's changed files, landed unless the editor did; the first refusal stops them all.
fn land_changed_files(
    workspace: &dyn Workspace,
    plan: &Plan,
    landed: ChangesLanded,
) -> Result<Vec<String>, (String, WriteRefused)> {
    let changed = plan.changed_files();
    if landed == ChangesLanded::NotYet {
        for file in &changed {
            workspace
                .write_changed(Path::new(&file.path), &file.before, &file.text)
                .map_err(|refused| (file.path.clone(), refused))?;
        }
    }
    Ok(changed.iter().map(|file| file.path.clone()).collect())
}

/// Writes the plan's new files in order until one is refused.
fn write_new_files(
    workspace: &dyn Workspace,
    plan: &Plan,
) -> (Vec<String>, Option<(String, WriteRefused)>) {
    let mut written = Vec::new();
    for file in plan.new_files() {
        if let Err(refused) = workspace.write_new(Path::new(&file.path), &file.text) {
            return (written, Some((file.path.clone(), refused)));
        }
        written.push(file.path.clone());
    }
    (written, None)
}
