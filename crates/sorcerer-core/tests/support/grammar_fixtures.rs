//! The vendored grammars: languages and a cache that need no network.

use std::path::Path;

use tempfile::TempDir;

/// The shipped languages with each grammar pointed at its test sources (vendored or fetched by
/// `scripts/test-grammars`).
pub fn languages_with_vendored_grammars() -> sorcerer_core::domain::language::Languages {
    let d = TempDir::new().unwrap();
    let mut ls = sorcerer_core::domain::language::Languages::assemble(d.path(), &[]).unwrap();
    super::point_at_test_grammars(&mut ls);
    ls
}

/// A grammar cache shared by every test in this process (built once).
pub fn test_cache() -> sorcerer_core::adapters::grammar::GrammarCache {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("grammar-cache");
    sorcerer_core::adapters::grammar::GrammarCache::at(dir)
}
