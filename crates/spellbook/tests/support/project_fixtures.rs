//! A project on the vendored grammars.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::language::{GrammarSource, Languages, Loaded};
use sorcerer_core::domain::project::Project;

/// Core's vendored grammar sources for a language (no network).
pub fn vendored(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sorcerer-core/tests/grammars")
        .join(name)
}

/// A grammar cache shared by every test in this process (built once).
pub fn test_cache() -> GrammarCache {
    GrammarCache::at(Path::new(env!("CARGO_TARGET_TMPDIR")).join("grammar-cache"))
}

/// The project at a directory with its grammars pointed at the vendored sources.
pub fn vendored_project(dir: &Path) -> Project {
    let mut project = Project::open(dir).unwrap();
    point_at_vendored(&mut project.languages);
    project
}

/// The project at a directory with its grammars pointed at the vendored sources, and a view of
/// it.
pub fn project_with_vendored_grammars(dir: &Path) -> (Arc<Project>, FilesView) {
    let project = Arc::new(vendored_project(dir));
    let view = FilesView::new(project.clone(), test_cache());
    (project, view)
}

/// The grammars of a set of languages pointed at the vendored sources.
pub fn point_at_vendored(languages: &mut Languages) {
    for name in ["python", "rust"] {
        languages.override_grammar(name, GrammarSource::Local(vendored(name)));
    }
}

/// A shipped language loaded from the vendored grammar.
pub fn loaded(name: &str) -> Arc<Loaded> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".sorcerer")).unwrap();
    vendored_project(dir.path())
        .languages
        .load(name, &test_cache())
        .unwrap()
}

/// The real adapters for a project on the vendored grammars and a fixed clock.
pub fn wired_at(dir: &Path) -> spellbook::adapters::wiring::Wired {
    spellbook::adapters::wiring::wired(
        vendored_project(dir),
        test_cache(),
        Arc::new(super::FixedClock::at_noon()),
    )
    .unwrap()
}

/// A corpus's rules file copied into a project as `.sorcerer/spellbook/rules.toml`, with the
/// corpus's generators (its rules' fixes name them).
pub fn use_corpus_rules(root: &Path, corpus: &str) {
    let spellbook = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/spellbook")
        .join(corpus)
        .join("spellbook");
    let to = root.join(".sorcerer/spellbook/rules.toml");
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(spellbook.join("rules.toml"), to).unwrap();
    let generators = root.join(".sorcerer/spellbook/generators");
    std::fs::create_dir_all(&generators).unwrap();
    super::copy_tree(&spellbook.join("generators"), &generators);
}
