//! A file's shape from its language's grammar and queries: the comments `textobjects.scm`
//! captures (`@comment.outer`), the paragraphs and headings `prose.scm` captures (`@prose`,
//! `@heading`), the units, and which units do nothing yet (core's facts). No language is named
//! here: a language without the captures has no comments or prose, and a file no language
//! covers, or whose grammar fails, is prose.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::captures::{COMMENT, Captures, HEADING, PROSE};
use sorcerer_core::domain::facts::facts_of;
use sorcerer_core::domain::language::{CommentMarks, Languages, Loaded};
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
    /// A file whose grammar does not load is shaped as one no language covers: prose.
    fn shape(&self, file: &Path, text: &str) -> Shape {
        let Some(loaded) = self.loaded_for(file) else {
            return Shape::default();
        };
        let Ok(Some(tree)) = loaded.parse(text) else {
            return Shape::default();
        };
        let captures = Captures::of(&loaded, &tree, text);
        Shape {
            comments: lines_of(&captures, COMMENT),
            prose: lines_of(&captures, PROSE),
            headings: lines_of(&captures, HEADING),
            units: units_with_stubs(&loaded, text),
            has_language: true,
        }
    }

    fn comment_marks(&self, file: &Path) -> Option<CommentMarks> {
        self.languages.for_path(file)?.comment_marks()
    }
}

/// The lines of each node captured under the name.
fn lines_of(captures: &Captures, name: &str) -> Vec<Span> {
    captures
        .nodes(&[name])
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
