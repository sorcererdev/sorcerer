//! The holes still open: each saved place looked for again in its file.

use std::path::{Path, PathBuf};

use sorcerer_core::ports::view::CodebaseView;

use crate::domain::open_hole_places::still_open;
use crate::domain::record::{OpenHole, sorted_open_holes};
use crate::ports::holes_store::HolesStore;

/// What finding the open holes reads.
pub struct HolesDeps<'a> {
    pub view: &'a dyn CodebaseView,
    pub holes: &'a dyn HolesStore,
}

/// The open holes, in all files or only in the given ones, by file then line. Each saved place
/// is looked for again in its file: kept at its current line while its placeholder is there,
/// dropped when the placeholder or the file is gone. The saved holes are rewritten with what is
/// still open, unless a file was there but could not be read (then nothing is forgotten).
///
/// Example: after `new command`, filling `handle` leaves `field` (twice) and `type` open.
pub fn open_holes(
    deps: &HolesDeps,
    files: Option<&[PathBuf]>,
) -> sorcerer_core::Result<Vec<OpenHole>> {
    let saved = deps.holes.read()?;
    let in_scope =
        |hole: &OpenHole| files.is_none_or(|files| files.iter().any(|f| same_file(f, &hole.file)));
    let mut every_file_read = true;
    let mut kept = Vec::new();
    let mut found = Vec::new();
    for hole in saved {
        if !in_scope(&hole) {
            kept.push(hole);
            continue;
        }
        match look_again(deps.view, &hole) {
            Looked::Open(now) => {
                found.push((*now).clone());
                kept.push(*now);
            }
            Looked::Gone => {}
            // a stash or a branch switch may bring the file back: keep the row, list nothing
            Looked::FileMissing => kept.push(hole),
            Looked::Unreadable => {
                every_file_read = false;
                kept.push(hole);
            }
        }
    }
    if every_file_read {
        deps.holes.replace(sorted_open_holes(kept))?;
    }
    Ok(sorted_open_holes(found))
}

enum Looked {
    Open(Box<OpenHole>),
    Gone,
    FileMissing,
    Unreadable,
}

fn look_again(view: &dyn CodebaseView, hole: &OpenHole) -> Looked {
    let path = Path::new(&hole.file);
    if !view.exists(path) {
        return Looked::FileMissing;
    }
    let Ok(text) = view.text(path) else {
        return Looked::Unreadable;
    };
    match still_open(hole, &text) {
        Some(line) => Looked::Open(Box::new(OpenHole {
            line,
            ..hole.clone()
        })),
        None => Looked::Gone,
    }
}

/// Whether a path given on the command line names the hole's file (relative, `/` or `\`).
fn same_file(given: &Path, file: &str) -> bool {
    let given = given.to_string_lossy().replace('\\', "/");
    given.trim_start_matches("./") == file
}
