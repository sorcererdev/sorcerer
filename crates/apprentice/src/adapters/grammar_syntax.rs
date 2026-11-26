//! A file's shape from its language's grammar and queries: the comments `textobjects.scm`
//! captures (`@comment.outer`), the units, and which units do nothing yet (core's facts). No
//! language is named here: a language without the captures has no comments, and a file no
//! language covers is prose.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::captures::{COMMENT, Captures};
use sorcerer_core::domain::facts::facts_of;
use sorcerer_core::domain::language::{Languages, Loaded};
use sorcerer_core::domain::unit::{Span, units_of};

use crate::domain::cutting::{Shape, ShapeUnit};
use crate::ports::syntax::Syntax;

pub struct GrammarSyntax {
    pub languages: Languages,
    pub cache: GrammarCache,
}

impl GrammarSyntax {
    fn loaded_for(&self, file: &Path) -> Option<Arc<Loaded>> {
        let language = self.languages.for_path(file)?;
        self.languages.load(&language.name, &self.cache).ok()
    }
}

impl Syntax for GrammarSyntax {
    fn shape(&self, file: &Path, text: &str) -> Shape {
        let Some(loaded) = self.loaded_for(file) else {
            return Shape {
                has_language: self.languages.for_path(file).is_some(),
                ..Shape::default()
            };
        };
        Shape {
            comments: comments_of(&loaded, text),
            units: units_with_stubs(&loaded, text),
            has_language: true,
        }
    }

    fn comment_prefix(&self, file: &Path) -> Option<String> {
        self.languages.for_path(file)?.comment.clone()
    }
}

fn comments_of(loaded: &Loaded, text: &str) -> Vec<Span> {
    let Ok(Some(tree)) = loaded.parse(text) else {
        return Vec::new();
    };
    Captures::of(loaded, &tree, text)
        .nodes(&[COMMENT])
        .iter()
        .map(|node| Span {
            start: node.start_position().row as u32 + 1,
            end: node.end_position().row as u32 + 1,
        })
        .collect()
}

fn units_with_stubs(loaded: &Loaded, text: &str) -> Vec<ShapeUnit> {
    let units = units_of(loaded, text).unwrap_or_default();
    units
        .iter()
        .map(|unit| ShapeUnit {
            name: unit.name.clone(),
            lines: unit.span,
            is_stub: facts_of(loaded, text, unit).is_ok_and(|facts| facts.body_empty),
        })
        .collect()
}
