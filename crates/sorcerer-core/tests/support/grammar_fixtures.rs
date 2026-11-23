//! The vendored grammars: languages and a cache that need no network.

use std::path::Path;

use tempfile::TempDir;

/// The vendored grammar sources for NAME (no network).
pub fn vendored(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/grammars")
        .join(name)
}

/// The shipped languages with each grammar pointed at its vendored sources.
pub fn languages_with_vendored_grammars() -> sorcerer_core::domain::language::Languages {
    let d = TempDir::new().unwrap();
    let mut ls = sorcerer_core::domain::language::Languages::assemble(d.path(), &[]).unwrap();
    for name in ["python", "rust"] {
        ls.override_grammar(
            name,
            sorcerer_core::domain::language::GrammarSource::Local(vendored(name)),
        );
    }
    ls
}

/// A grammar cache shared by every test in this process (built once).
pub fn test_cache() -> sorcerer_core::adapters::grammar::GrammarCache {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("grammar-cache");
    sorcerer_core::adapters::grammar::GrammarCache::at(dir)
}
