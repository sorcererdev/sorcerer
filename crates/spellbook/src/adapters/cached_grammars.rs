//! The grammars port over the project's languages and a grammar cache: each grammar is
//! downloaded or built once into the cache, and loaded once per run.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::Result;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::language::{Languages, Loaded};

use crate::ports::grammars::Grammars;

pub struct CachedGrammars {
    pub languages: Languages,
    pub cache: GrammarCache,
}

impl Grammars for CachedGrammars {
    fn for_path(&self, file: &Path) -> Result<Option<Arc<Loaded>>> {
        self.languages
            .for_path(file)
            .map(|language| self.languages.load(&language.name, &self.cache))
            .transpose()
    }

    /// Read from the language's data: no grammar is loaded.
    fn covers(&self, file: &Path) -> bool {
        self.languages.for_path(file).is_some()
    }

    /// Read from the language's data: no grammar is loaded.
    fn line_comment(&self, file: &Path) -> Option<String> {
        self.languages.for_path(file)?.comment.clone()
    }
}
