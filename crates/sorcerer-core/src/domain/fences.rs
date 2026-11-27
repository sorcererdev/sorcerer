//! Fences: the paths a researcher may never read or send to a model.
//! Secrets (`.env*` anywhere), sorcerer's own state (`.sorcerer/`, which
//! holds other findings and the ledger), git's internals, anything outside
//! the root (a symlink out, `..`), and the user's `[model] not_evidence`
//! globs. A path is resolved (symlinks, `..`) before it is matched, and
//! matching ignores case. When a path cannot be resolved for certain, it is
//! fenced.
//!
//! One place inside `.sorcerer/` is not fenced: the notes, `.sorcerer/notes/`. They are the
//! person's and the lackey's working files, read and worked on like any other file. The
//! carve-out is judged on the resolved path, so a link inside the notes onto `.sorcerer/state/`
//! stays fenced, and the other fences (`.env*`, `not_evidence`) still hold inside the notes.

use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::{Error, Result};

/// Paths fenced in every project: secret files (`.env`, `.env.local`, … anywhere), sorcerer's
/// own state, and git's internals.
pub const ALWAYS: &[&str] = &["**/.env*", SORCERER_DIR_FENCE, ".git/**"];

/// The fence over sorcerer's own directory, which the notes are carved out of.
pub const SORCERER_DIR_FENCE: &str = ".sorcerer/**";

/// Where notes live, relative to the root: the one directory under `.sorcerer/` that is not
/// fenced.
pub const NOTES_DIR: &str = ".sorcerer/notes";

/// The paths of one project that no model may read or be sent.
#[derive(Debug, Clone)]
pub struct Fences {
    pub(crate) root: PathBuf,
    /// Every fence but sorcerer's own directory.
    globs: GlobSet,
    /// `.sorcerer/**`, which holds everywhere but in the notes.
    sorcerer_dir: GlobSet,
    patterns: Vec<String>,
}

impl Fences {
    /// The fences of the project at `root` (a path with its symlinks already resolved): the
    /// [`ALWAYS`] patterns plus the user's `[model] not_evidence` entries from
    /// `.sorcerer/config.toml`.
    ///
    /// An entry with no glob characters names a file or a directory and fences everything under
    /// it too, so `docs/plans` fences `docs/plans/today.md`. Matching ignores case. An entry that
    /// is not a valid glob is a config error.
    pub fn new(root: PathBuf, not_evidence: &[String]) -> Result<Fences> {
        let patterns: Vec<String> = ALWAYS
            .iter()
            .map(|pattern| pattern.to_string())
            .chain(not_evidence.iter().flat_map(|entry| directory_style(entry)))
            .collect();
        let (sorcerer_dir, others): (Vec<String>, Vec<String>) = patterns
            .iter()
            .cloned()
            .partition(|pattern| pattern == SORCERER_DIR_FENCE);
        Ok(Fences {
            root,
            globs: case_insensitive_globs(&others)?,
            sorcerer_dir: case_insensitive_globs(&sorcerer_dir)?,
            patterns,
        })
    }

    /// Whether a path, relative to the root and already resolved, may be read and sent.
    /// The empty path (the root itself) never may. [`Fences::allows`] takes any path and
    /// resolves it first.
    ///
    /// Example: `.sorcerer/notes/plan.md` may be; `.sorcerer/state/ledger.jsonl`,
    /// `.sorcerer/notes/.env` and `.sorcerer/notes/../config.toml` (not resolved) may not.
    pub fn allows_relative(&self, rel: &Path) -> bool {
        !rel.as_os_str().is_empty()
            && !self.globs.is_match(rel)
            && (is_in_notes(rel) || !self.sorcerer_dir.is_match(rel))
    }

    /// The fence patterns as text, to turn into `claude`'s deny rules (see
    /// `Fences::deny_patterns`, which carves the notes out of `.sorcerer/**`) and to tell one
    /// walk's fences from another's.
    pub fn patterns(&self) -> Vec<String> {
        self.patterns.clone()
    }

    /// The fences as a model is told them: the patterns, with the notes named as the exception
    /// to `.sorcerer/**`.
    ///
    /// Example: `**/.env*, .sorcerer/** (except .sorcerer/notes/), .git/**`.
    pub fn described(&self) -> String {
        self.patterns
            .iter()
            .map(|pattern| match pattern.as_str() {
                SORCERER_DIR_FENCE => format!("{SORCERER_DIR_FENCE} (except {NOTES_DIR}/)"),
                _ => pattern.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The project root the fences are relative to.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Whether a resolved path, relative to the root, is the notes directory or inside it. Names
/// are compared ignoring case, as the fences are; a path with `.` or `..` in it never is (the
/// path must be resolved first).
///
/// Example: `.sorcerer/notes/plan.md` and `.Sorcerer/Notes` are; `.sorcerer/notes2/x.md`,
/// `.sorcerer/state/notes/x.md` and `.sorcerer/notes/../state` are not.
pub fn is_in_notes(rel: &Path) -> bool {
    let names: Option<Vec<&std::ffi::OsStr>> = rel
        .components()
        .map(|component| match component {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    let Some(names) = names else {
        return false;
    };
    let notes = NOTES_DIR.split('/');
    names.len() >= 2
        && names.iter().zip(notes).all(|(name, wanted)| {
            name.to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
        })
}

/// The patterns for one `not_evidence` entry. A glob stays as it is; a plain path (`docs/plans`,
/// `notes/`) becomes two patterns, the path itself and everything under it:
/// `["docs/plans", "docs/plans/**"]`.
fn directory_style(entry: &str) -> Vec<String> {
    if entry.contains(['*', '?', '[', '{']) {
        return vec![entry.to_string()];
    }
    let bare = entry.trim_end_matches('/');
    vec![bare.to_string(), format!("{bare}/**")]
}

/// One glob set matching any of the patterns, ignoring case; `*` also crosses `/`.
fn case_insensitive_globs(patterns: &[String]) -> Result<GlobSet> {
    let globs = patterns
        .iter()
        .try_fold(GlobSetBuilder::new(), |mut set, pattern| {
            set.add(case_insensitive_glob(pattern)?);
            Ok::<_, Error>(set)
        })?;
    globs.build().map_err(|e| config_error(e.to_string()))
}

fn case_insensitive_glob(pattern: &str) -> Result<globset::Glob> {
    GlobBuilder::new(pattern)
        .case_insensitive(true)
        .literal_separator(false)
        .build()
        .map_err(|e| config_error(format!("[model] not_evidence: {e}")))
}

fn config_error(why: String) -> Error {
    Error::Config {
        file: PathBuf::from(".sorcerer/config.toml"),
        why,
    }
}
