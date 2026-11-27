//! The project's own files as conjure's context: imports by the language's query, parse errors
//! by its grammar, scry's findings file and steering file.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::Result;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::facts::parse_errors_of;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::Loaded;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::view::CodebaseView;

use crate::adapters::imports::related_files;
use crate::adapters::names_in_text::names_in;
use crate::adapters::scry_files::read_findings;
use crate::adapters::steering_files::project_rules;
use crate::domain::retry_drops::NamedInText;
use crate::domain::scry_notes::FindingsRead;
use crate::ports::project_context::{ProjectContext, RelatedFiles};

pub struct FilesContext {
    pub project: Arc<Project>,
    /// Reads the related files (the editor's text laid over the disk, in the server).
    pub view: Arc<dyn CodebaseView>,
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
}

impl ProjectContext for FilesContext {
    fn related_files(&self, file: &Path, text: &str) -> Result<RelatedFiles> {
        match self.loaded_for(file) {
            Some(loaded) => related_files(&*self.view, &loaded, file, text, &|path| {
                self.fences.allows(path)
            }),
            None => Ok(RelatedFiles::default()),
        }
    }

    fn parse_errors(&self, file: &Path, text: &str) -> Option<Vec<u32>> {
        let loaded = self.loaded_for(file)?;
        parse_errors_of(&loaded, text).ok()
    }

    fn names_in(&self, file: &Path, text: &str) -> Option<Vec<NamedInText>> {
        let loaded = self.loaded_for(file)?;
        Some(names_in(&loaded, text))
    }

    fn scry_findings(&self) -> FindingsRead {
        read_findings(&self.project).unwrap_or_else(|e| FindingsRead::OtherVersion(e.to_string()))
    }

    fn rules(&self) -> String {
        project_rules(&self.project, &self.fences)
    }
}
