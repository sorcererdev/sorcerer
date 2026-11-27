//! Checking what a branch touched: the rules' findings on the lines changed since a commit, and
//! every region.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sorcerer_core::Result;
use sorcerer_core::domain::paths::forward_slashes;

use crate::domain::change::added_lines;
use crate::domain::finding::Finding;
use crate::domain::report::sort_findings;
use crate::usecases::check_files::{CheckDeps, regions_out_of_step, rule_findings_of_files};
use crate::usecases::check_project::is_spellbooks_own;

/// Checks the files changed since `rev` (anything git knows as a commit: `main`, `HEAD~3`) and
/// keeps the rules' findings on lines added or changed since then; a file `rev` did not have is
/// all new. Every region is checked and its findings kept, whatever lines changed: a new file
/// makes a list stale without touching it. The baseline marks what is existing over the changed
/// files' whole findings, untouched lines first, before the lines are kept: a pasted copy of
/// baselined code is new, as a full check says. Nothing is saved: the report keeps whole checks.
/// Returns the findings, by file then line.
///
/// Example: a finding committed in `a.x` on `main` and a new one written in `b.x` on
/// the branch: `check_since(deps, "main")` returns the one in `b.x`.
pub fn check_since(deps: &CheckDeps, rev: &str) -> Result<Vec<Finding>> {
    let files = changed_files_on_disk(deps, rev)?;
    let added = added_lines_of(deps, &files, rev)?;
    let mut whole = rule_findings_of_files(deps, &files)?;
    sort_findings(&mut whole);
    let mut found: Vec<Finding> = deps
        .baseline
        .marked_unchanged_first(whole, |finding| !on_added_line(&added, finding))
        .into_iter()
        .filter(|finding| on_added_line(&added, finding))
        .collect();
    found.extend(regions_out_of_step(deps, None)?);
    sort_findings(&mut found);
    Ok(found)
}

/// The files that differ from `rev`, or are new, and are still there.
fn changed_files_on_disk(deps: &CheckDeps, rev: &str) -> Result<Vec<PathBuf>> {
    Ok(deps
        .view
        .changed_since(rev)?
        .into_iter()
        .filter(|file| !is_spellbooks_own(file) && deps.view.exists(file))
        .collect())
}

/// Each file's added lines, by its name with `/`; `None` when `rev` did not have the file.
fn added_lines_of(
    deps: &CheckDeps,
    files: &[PathBuf],
    rev: &str,
) -> Result<BTreeMap<String, Option<BTreeSet<u32>>>> {
    files
        .iter()
        .map(|file| Ok((forward_slashes(file), added_lines_in(deps, file, rev)?)))
        .collect()
}

fn added_lines_in(deps: &CheckDeps, file: &Path, rev: &str) -> Result<Option<BTreeSet<u32>>> {
    let Some(before) = deps.view.text_at(file, rev)? else {
        return Ok(None);
    };
    let Ok(now) = deps.view.text(file) else {
        return Ok(Some(BTreeSet::new()));
    };
    Ok(Some(added_lines(&before, &now)))
}

fn on_added_line(added: &BTreeMap<String, Option<BTreeSet<u32>>>, finding: &Finding) -> bool {
    match added.get(&finding.file) {
        Some(None) => true,
        Some(Some(lines)) => lines.contains(&finding.line),
        None => false,
    }
}
