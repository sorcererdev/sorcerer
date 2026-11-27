//! The vendored grammars: a project that builds its grammars from core's test sources, in a
//! cache shared by the tests of one process. No network.

use std::path::Path;

use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::project::Project;

pub fn test_cache() -> GrammarCache {
    GrammarCache::at(Path::new(env!("CARGO_TARGET_TMPDIR")).join("grammar-cache"))
}

/// The project at ROOT with its grammars pointed at the vendored sources.
pub fn open_with_vendored_grammars(root: &Path) -> anyhow::Result<Project> {
    let mut project = Project::open(root)?;
    super::point_at_test_grammars(&mut project.languages);
    Ok(project)
}
