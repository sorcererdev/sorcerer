//! A file's shape through the real grammars: what lackey's cutting is given.

mod support;

use std::path::Path;

use lackey::adapters::grammar_syntax::GrammarSyntax;
use lackey::domain::cutting::Shape;
use lackey::ports::syntax::Syntax;
use support::*;

fn syntax_of(project: &TempProject) -> GrammarSyntax {
    GrammarSyntax {
        languages: open_with_vendored_grammars(&project.root)
            .unwrap()
            .languages,
        cache: test_cache(),
    }
}

#[test]
fn a_file_whose_grammar_does_not_load_is_shaped_as_prose() {
    let project = TempProject::with(&[(
        ".sorcerer/languages/notes/language.toml",
        "files = [\"**/*.notes\"]\ngrammar = { path = \"missing\" }\ncomment = \"#\"\n",
    )]);
    let shape = syntax_of(&project).shape(Path::new("a.notes"), "Why?\n");
    assert_eq!(shape, Shape::default());
}

#[test]
fn prose_and_headings_come_from_the_languages_captures() {
    // a Python comment stands in for a heading and a string for a paragraph
    let project = TempProject::with(&[(
        ".sorcerer/languages/python/queries/prose.scm",
        "(module (expression_statement (string) @prose))\n(module (comment) @heading)\n",
    )]);
    let text = "# Title\n\"\"\"A paragraph\nof two lines.\"\"\"\n";
    let shape = syntax_of(&project).shape(Path::new("a.py"), text);
    assert!(shape.has_language);
    let lines = |spans: &[sorcerer_core::domain::unit::Span]| -> Vec<(u32, u32)> {
        spans.iter().map(|s| (s.start, s.end)).collect()
    };
    assert_eq!(lines(&shape.headings), [(1, 1)]);
    assert_eq!(lines(&shape.prose), [(2, 3)]);
}
