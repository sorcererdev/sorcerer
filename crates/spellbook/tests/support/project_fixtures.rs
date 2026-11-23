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
