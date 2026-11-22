//! Checking what the writer wrote: which paths may land, what each file's edits are, whether
//! each file parses. Nothing is refused here beyond the paths; the facts are the mark's words.

use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use sorcerer_core::domain::paths::forward_slashes;

use super::{ChangeDeps, ChangeRefused, ChangedFile};
use crate::domain::edits::{
    DropReason, Landing, first_changed_line, in_base_line_endings, lexical_relative, line_edits,
    lines_changed,
};
use crate::domain::reply::ChangeReply;

/// The files that land, and the paths dropped with why.
pub(super) type CheckedFiles = (Vec<ChangedFile>, Vec<(String, DropReason)>);

/// The files of a reply that land, each as edits against its base text, and the paths dropped
/// with why. A reply that says nothing, or whose every file equals its base, has no files.
pub(super) fn checked_files(
    reply: &ChangeReply,
    deps: &ChangeDeps,
) -> Result<CheckedFiles, ChangeRefused> {
    if reply.is_nothing() {
        return Ok((Vec::new(), Vec::new()));
    }
    let excluded = exclude_globs(deps)?;
    let mut files = Vec::new();
    let mut dropped = Vec::new();
    for (written, text) in &reply.files {
        match landing_of(written, &excluded, deps) {
            Landing::Dropped { path, why } => dropped.push((path, why)),
            Landing::Lands(path) => match changed_file(path, text, deps) {
                Err(why) => dropped.push((written.clone(), why)),
                Ok(Some(file)) => files.push(file),
                Ok(None) => {}
            },
        }
    }
    Ok((files, dropped))
}

fn exclude_globs(deps: &ChangeDeps) -> Result<GlobSet, ChangeRefused> {
    let mut builder = GlobSetBuilder::new();
    for pattern in &deps.config.exclude {
        let glob = Glob::new(pattern).map_err(|e| ChangeRefused::Failed(e.to_string()))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| ChangeRefused::Failed(e.to_string()))
}

/// Whether a written path may land: inside the project after clean-up, past the fences
/// (symlinks resolved), and matching no `exclude` glob.
fn landing_of(written: &str, excluded: &GlobSet, deps: &ChangeDeps) -> Landing {
    let dropped = |why| Landing::Dropped {
        path: written.to_string(),
        why,
    };
    let Some(relative) = lexical_relative(&deps.project.root, written) else {
        return dropped(DropReason::OutsideProject);
    };
    if !deps.fences.allows(&relative) {
        return dropped(DropReason::Fenced);
    }
    if excluded.is_match(forward_slashes(&relative)) {
        return dropped(DropReason::Excluded);
    }
    Landing::Lands(relative)
}

/// One file as edits against what is there now. `Ok(None)` when it equals its base; an error
/// when what is there cannot be read as text.
fn changed_file(
    path: PathBuf,
    written: &str,
    deps: &ChangeDeps,
) -> Result<Option<ChangedFile>, DropReason> {
    let before = if deps.view.exists(&path) {
        Some(deps.view.text(&path).map_err(|_| DropReason::NotText)?)
    } else {
        None
    };
    let base = before.as_deref().unwrap_or("");
    let after = in_base_line_endings(base, written);
    let edits = line_edits(base, &after);
    let Some(first) = first_changed_line(base, &after) else {
        return Ok(None);
    };
    let (parsed, parse_errors) = parse_errors_of_file(&path, &after, deps);
    Ok(Some(ChangedFile {
        new: before.is_none(),
        before,
        lines_changed: lines_changed(&edits),
        first_changed_line: first,
        edits,
        after,
        parsed,
        parse_errors,
        path,
    }))
}

/// Whether a grammar covers the file, and the lines where its text does not parse. A file of
/// no known language is kept and reported unparsed.
fn parse_errors_of_file(path: &Path, text: &str, deps: &ChangeDeps) -> (bool, Vec<u32>) {
    match deps.context.parse_errors(path, text) {
        Some(lines) => (true, lines),
        None => (false, Vec::new()),
    }
}
