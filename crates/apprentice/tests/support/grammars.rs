//! The vendored grammars: a project that builds its grammars from core's test sources, in a
//! cache shared by the tests of one process. No network.

use std::path::{Path, PathBuf};

use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::language::GrammarSource;
use sorcerer_core::domain::project::Project;

/// Core's vendored grammar sources for NAME.
pub fn vendored(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sorcerer-core/tests/grammars")
        .join(name)
}

pub fn test_cache() -> GrammarCache {
    GrammarCache::at(Path::new(env!("CARGO_TARGET_TMPDIR")).join("grammar-cache"))
}

/// The project at ROOT with its grammars pointed at the vendored sources.
pub fn open_with_vendored_grammars(root: &Path) -> anyhow::Result<Project> {
    let mut project = Project::open(root)?;
    for name in ["python", "rust"] {
        project
            .languages
            .override_grammar(name, GrammarSource::Local(vendored(name)));
    }
    Ok(project)
}
