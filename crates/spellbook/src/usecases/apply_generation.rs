//! A plan applied: its new files written, the generation recorded, the open holes kept.

use std::path::Path;

use crate::domain::plan::{FileStatus, Plan};
use crate::domain::record::{open_holes_of, record_of, sorted_open_holes};
use crate::ports::clock::Clock;
use crate::ports::generation_store::GenerationStore;
use crate::ports::holes_store::HolesStore;
use crate::ports::workspace::{WriteRefused, Workspace};

/// What applying writes to.
pub struct ApplyDeps<'a> {
    pub workspace: &'a dyn Workspace,
    pub generations: &'a dyn GenerationStore,
    pub holes: &'a dyn HolesStore,
    pub clock: &'a dyn Clock,
}

/// What applying did: the files written and skipped, and the file refused, if one was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedGeneration {
    pub plan: Plan,
    pub written: Vec<String>,
    pub skipped: Vec<String>,
    /// The file that stopped the writing, and why; the files after it were not written.
    pub refused: Option<(String, WriteRefused)>,
}

/// Applies a plan: writes its new files in order, stopping at the first refusal (the files
/// already written stay: each is a whole new file the person can delete), then appends the
/// generation's record for what was written, then adds the placeholder places in the written
/// files to the open holes.
///
/// Example: if `cancel_table.py` appeared between the plan and now, it is refused, the test
/// file after it is not written, and the record names only what was.
pub fn apply_generation(deps: &ApplyDeps, plan: Plan) -> sorcerer_core::Result<AppliedGeneration> {
    let (written, refused) = write_new_files(deps.workspace, &plan);
    let skipped: Vec<String> = plan
        .files
        .iter()
        .filter(|file| file.status == FileStatus::Skipped)
        .map(|file| file.path.clone())
        .collect();
    let kept: Vec<String> = written.iter().chain(&skipped).cloned().collect();
    deps.generations
        .append(&record_of(&plan, deps.clock.now(), &kept))?;
    let mut open = deps.holes.read()?;
    open.extend(open_holes_of(&plan, &written));
    deps.holes.replace(sorted_open_holes(open))?;
    Ok(AppliedGeneration {
        plan,
        written,
        skipped,
        refused,
    })
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
