//! The project's own files as a task's context: the unit around a line by the language's
//! grammar, scry's findings file and steering file.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::Loaded;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::{Unit, unit_at, units_of};

use crate::adapters::scry_files::read_findings;
use crate::adapters::steering_files::project_rules;
use crate::domain::scry_notes::{FindingsRead, ScryNote, notes_near};
use crate::ports::project_context::ProjectContext;

pub struct FilesContext {
    pub project: Arc<Project>,
    pub cache: GrammarCache,
    pub fences: Fences,
}

impl FilesContext {
    /// The language of a file, loaded; none when no language covers it or its grammar fails.
    fn loaded_for(&self, file: &Path) -> Option<Arc<Loaded>> {
        let language = self.project.languages.for_path(file)?;
        self.project
            .languages
            .load(&language.name, &self.cache)
            .ok()
    }

    fn unit_of(&self, file: &Path, text: &str, line: u32) -> Option<Unit> {
        let loaded = self.loaded_for(file)?;
        let units = units_of(&loaded, text).ok()?;
        unit_at(&units, line).cloned()
    }
}

impl ProjectContext for FilesContext {
    fn unit_at(&self, file: &Path, text: &str, line: u32) -> Option<(String, String)> {
        let unit = self.unit_of(file, text, line)?;
        let lines: Vec<&str> = text
            .lines()
            .skip(unit.span.start.saturating_sub(1) as usize)
            .take((unit.span.end + 1 - unit.span.start) as usize)
            .collect();
        Some((unit.name, lines.join("\n")))
    }

    fn notes_near(&self, file: &Path, text: &str, line: u32) -> Vec<ScryNote> {
        let Some(unit) = self.unit_of(file, text, line) else {
            return Vec::new();
        };
        let FindingsRead::Found(findings) =
            read_findings(&self.project).unwrap_or(FindingsRead::NoFile)
        else {
            return Vec::new();
        };
        let fingerprint_now = |path: &Path, span: sorcerer_core::domain::unit::Span| {
            (path == file)
                .then(|| self.unit_of(file, text, span.start))
                .flatten()
                .map(|unit| unit.fingerprint.as_str().to_string())
        };
        notes_near(&findings, &unit, file, None, fingerprint_now)
    }

    fn rules(&self) -> String {
        project_rules(&self.project, &self.fences)
    }
}
