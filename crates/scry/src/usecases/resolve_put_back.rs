//! Findings on code put back to the base, resolved without a review call, whichever way the
//! code came back: an undo and a save, or `git checkout` with no save at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::versioned::Snapshot;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Error, Result};

use crate::domain::config::ScryConfig;
use crate::domain::finding::{FindingId, Status};
use crate::domain::findings::Findings;
use crate::domain::placing::placed;
use crate::domain::scope::ScopeRequest;
use crate::ports::finding_store::FindingStore;
use crate::usecases::review_change::scope::scope_of;

/// Where to look for findings put back, and what with.
pub struct PutBackLook<'a> {
    pub project: &'a Project,
    pub view: &'a dyn CodebaseView,
    pub store: &'a dyn FindingStore,
    pub cfg: &'a ScryConfig,
    /// When a finding resolved here is dated.
    pub at: DateTime<Utc>,
}

impl PutBackLook<'_> {
    /// Resolves the open and unverified findings, in the files given (relative to the
    /// project), whose unit is the same now as at `HEAD` but not the code the finding was
    /// made on; returns their ids. No model is called.
    ///
    /// 1. Only the files with a finding that is stale now (its unit's code changed since the
    ///    review) are looked at further, so a file with nothing stale costs no git call.
    /// 2. Their units are compared with `HEAD`, as a pass over those files would
    ///    (`scope_of`).
    /// 3. The findings on a unit put back (`Findings::put_back`) are resolved in the store.
    ///
    /// Example: a pass raised "cancel ignores now" on `cancel` as you had changed it. You ran
    /// `git checkout -- orders.py`, so `cancel` is what `HEAD` has again, and no pass would
    /// look at the file (it is no longer changed). Looking at `orders.py` resolves the
    /// finding; the editor shows nothing there.
    pub fn resolve(&self, files: &[PathBuf]) -> Result<Vec<FindingId>> {
        let stored = match self.store.read()? {
            Snapshot::Current(stored) => stored,
            Snapshot::Absent => return Ok(Vec::new()),
            Snapshot::Unreadable(why) => {
                return Err(Error::StoreUnreadable {
                    path: self.store.path().to_path_buf(),
                    why,
                });
            }
        };
        let stale_files = self.files_with_stale_findings(&stored, files);
        if stale_files.is_empty() {
            return Ok(Vec::new());
        }
        let scope = scope_of(
            self.project,
            self.view,
            ScopeRequest::Files(stale_files),
            false,
            self.cfg,
        )?;
        let mut resolved = Vec::new();
        self.store.update(&mut |findings| {
            resolved = findings.put_back(&scope);
            findings.close_for_changed_code(&resolved, Status::Resolved, self.at);
        })?;
        Ok(resolved)
    }

    /// The files, of those given, holding an open or unverified finding whose unit's code is
    /// not what was reviewed.
    fn files_with_stale_findings(&self, stored: &Findings, files: &[PathBuf]) -> Vec<PathBuf> {
        let candidates: BTreeSet<&Path> = stored
            .findings
            .iter()
            .filter(|f| matches!(f.status, Status::Open | Status::Unverified))
            .filter(|f| f.place.unit.is_some() && f.place.fingerprint.is_some())
            .map(|f| f.place.file.as_path())
            .filter(|file| files.iter().any(|given| given == file))
            .collect();
        candidates
            .into_iter()
            .filter(|file| self.has_a_stale_finding(stored, file))
            .map(Path::to_path_buf)
            .collect()
    }

    fn has_a_stale_finding(&self, stored: &Findings, file: &Path) -> bool {
        let Ok(units_now) = self.view.units(file) else {
            return false;
        };
        let line_count = self.view.text(file).map_or(0, |text| {
            u32::try_from(text.lines().count()).unwrap_or(u32::MAX)
        });
        stored
            .findings
            .iter()
            .filter(|f| f.place.file == file)
            .filter(|f| matches!(f.status, Status::Open | Status::Unverified))
            .any(|f| placed(f, &units_now, line_count).is_some_and(|p| p.stale))
    }
}
