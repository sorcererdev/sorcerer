//! The project as a plan so far would leave it: what `[[uses]]` plans over, so a used generator
//! sees the files the plan creates or changes before it (`add-port`'s `use-port` finds the port
//! folder `port` just planned).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use globset::GlobBuilder;
use sorcerer_core::Result;
use sorcerer_core::domain::facts::Facts;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::Unit;
use sorcerer_core::ports::view::{CodebaseView, GrepHit};

/// A view whose `exists`, `text` and `glob` answer with the planned files laid over the project;
/// every other question goes to the project as it is.
pub struct PlannedView<'v> {
    project: &'v dyn CodebaseView,
    /// The planned text of each new or changed file, by its path with `/`.
    planned: BTreeMap<String, String>,
}

impl<'v> PlannedView<'v> {
    pub fn new(
        project: &'v dyn CodebaseView,
        planned: BTreeMap<String, String>,
    ) -> PlannedView<'v> {
        PlannedView { project, planned }
    }

    fn planned_text(&self, path: &Path) -> Option<&String> {
        self.planned.get(&forward_slashes(path))
    }
}

impl CodebaseView for PlannedView<'_> {
    fn text(&self, path: &Path) -> Result<String> {
        match self.planned_text(path) {
            Some(text) => Ok(text.clone()),
            None => self.project.text(path),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.planned_text(path).is_some() || self.project.exists(path)
    }

    fn glob(&self, pattern: &str) -> Result<Vec<PathBuf>> {
        let mut found = self.project.glob(pattern)?;
        if let Ok(glob) = GlobBuilder::new(pattern).literal_separator(true).build() {
            let matcher = glob.compile_matcher();
            found.extend(
                self.planned
                    .keys()
                    .filter(|path| matcher.is_match(path.as_str()))
                    .map(PathBuf::from),
            );
        }
        found.sort();
        found.dedup();
        Ok(found)
    }

    fn text_at(&self, path: &Path, rev: &str) -> Result<Option<String>> {
        self.project.text_at(path, rev)
    }

    fn units(&self, path: &Path) -> Result<Vec<Unit>> {
        self.project.units(path)
    }

    fn unit_or_broken_region_at(&self, path: &Path, line: u32) -> Result<Option<Unit>> {
        self.project.unit_or_broken_region_at(path, line)
    }

    fn units_at(&self, path: &Path, rev: &str) -> Result<Vec<Unit>> {
        self.project.units_at(path, rev)
    }

    fn facts(&self, path: &Path, unit: &Unit) -> Result<Facts> {
        self.project.facts(path, unit)
    }

    fn grep(&self, pattern: &str, glob: Option<&str>) -> Result<Vec<GrepHit>> {
        self.project.grep(pattern, glob)
    }

    fn changed_since(&self, rev: &str) -> Result<Vec<PathBuf>> {
        self.project.changed_since(rev)
    }

    fn diff(&self, path: &Path, rev: &str) -> Result<String> {
        self.project.diff(path, rev)
    }

    fn merge_base(&self, rev: &str) -> Result<String> {
        self.project.merge_base(rev)
    }
}
