//! The session's changes from git, read through the project's view: the merge base with the
//! default branch (`origin/HEAD`, else `main`, else `master`), the files changed since it, and
//! each one's diff. A fenced file is left out, and a file git does not track yet reads as the
//! diff that adds it.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::triage_context::{FileDiff, SESSION_CHARS, SessionDiffs, added_file_diff};
use crate::ports::session_changes::SessionChanges;

/// The branches a session is measured from, in the order tried.
const DEFAULT_BRANCHES: [&str; 3] = ["origin/HEAD", "main", "master"];

pub struct GitSession {
    pub view: Arc<dyn CodebaseView>,
    pub fences: Fences,
}

impl SessionChanges for GitSession {
    /// 1. The merge base with the first default branch git knows; none, no changes.
    /// 2. The changed files the fences allow, `file` first.
    /// 3. Each one's diff, until they pass the budget; the rest are counted. (A file the
    ///    budget cannot hold is dropped later, by `session_changes_text`.)
    fn diffs(&self, file: &str) -> SessionDiffs {
        let Some(base) = self.merge_base() else {
            return SessionDiffs::default();
        };
        let paths = self.changed_files_allowed(&base, file);
        let mut diffs = SessionDiffs::default();
        let mut size = 0;
        for (n, path) in paths.iter().enumerate() {
            if size > SESSION_CHARS {
                diffs.left_out = paths.len() - n;
                break;
            }
            let diff = self.diff_of(path, &base);
            size += diff.len();
            if !diff.is_empty() {
                diffs.files.push(FileDiff {
                    path: path.clone(),
                    diff,
                });
            }
        }
        diffs
    }
}

impl GitSession {
    fn merge_base(&self) -> Option<String> {
        DEFAULT_BRANCHES
            .iter()
            .find_map(|branch| self.view.merge_base(branch).ok())
    }

    /// The changed files the fences allow, relative with `/`: `file` first when it changed.
    fn changed_files_allowed(&self, base: &str, file: &str) -> Vec<String> {
        let changed: Vec<String> = self
            .view
            .changed_since(base)
            .unwrap_or_default()
            .iter()
            .filter(|path| self.fences.allows(&self.fences.root().join(path)))
            .map(|path| forward_slashes(path))
            .collect();
        let (own, others): (Vec<String>, Vec<String>) =
            changed.into_iter().partition(|path| path == file);
        own.into_iter().chain(others).collect()
    }

    /// The file's diff from the base; for a file git does not track, the diff that adds it.
    fn diff_of(&self, path: &str, base: &str) -> String {
        let relative = Path::new(path);
        let diff = self.view.diff(relative, base).unwrap_or_default();
        if !diff.is_empty() || !self.view.exists(relative) {
            return diff;
        }
        match self.view.text_at(relative, base) {
            Ok(None) => self
                .view
                .text(relative)
                .map(|text| added_file_diff(path, &text))
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
}
