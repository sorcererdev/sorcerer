//! The grammars every crate's tests build from, with no network: each shipped language's
//! grammar pointed at its vendored sources (`crates/sorcerer-core/tests/grammars/<name>/`,
//! Python and Rust) or at the checkout `scripts/test-grammars` fetched
//! (`target/test-grammars/src/<name>-<rev7>/`, the rest).
//!
//! Core's tests use this file as a module; the other crates include the same file with
//! `#[path = "../../../sorcerer-core/tests/support/test_grammars.rs"]`, so there is one copy.

use std::path::{Path, PathBuf};

use sorcerer_core::domain::language::{GrammarSource, Language, Languages};

/// The repository's root: two folders above the crate whose test includes this file.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Core's vendored grammar sources for a language (they exist for Python and Rust only).
pub fn vendored(name: &str) -> PathBuf {
    repository_root()
        .join("crates/sorcerer-core/tests/grammars")
        .join(name)
}

/// Where the tests build a shipped language's grammar from: its vendored sources when there
/// are some, else the checkout `scripts/test-grammars` fetched (the folder `dir` names inside
/// it, for a repository holding two grammars). The checkout may be missing: a test that
/// loads the grammar then fails on that path, and `scripts/test-grammars` fetches it.
///
/// Example: `python` is `crates/sorcerer-core/tests/grammars/python`; a language pinned at
/// `5a5ca85…` is `target/test-grammars/src/<name>-5a5ca85`.
pub fn test_grammar_sources(language: &Language) -> PathBuf {
    let vendored = vendored(&language.name);
    if vendored.is_dir() {
        return vendored;
    }
    match &language.grammar {
        GrammarSource::Git { rev, dir, .. } => {
            let checkout = fetched_checkout(&language.name, rev);
            dir.as_ref()
                .map_or(checkout.clone(), |dir| checkout.join(dir))
        }
        GrammarSource::Local(dir) => dir.clone(),
    }
}

/// Where `scripts/test-grammars` puts a grammar's checkout.
pub fn fetched_checkout(name: &str, rev: &str) -> PathBuf {
    repository_root()
        .join("target/test-grammars/src")
        .join(format!("{name}-{}", &rev[..rev.len().min(7)]))
}

/// Points every shipped language's grammar at its test sources ([`test_grammar_sources`]),
/// so no test fetches or downloads one. A project's own language (a grammar the project
/// names) is left as it is.
pub fn point_at_test_grammars(languages: &mut Languages) {
    let shipped: Vec<(String, PathBuf)> = languages
        .names()
        .into_iter()
        .filter_map(|name| languages.get(name))
        .filter(|language| language.origins.get("grammar").map(String::as_str) == Some("shipped"))
        .map(|language| (language.name.clone(), test_grammar_sources(language)))
        .collect();
    for (name, sources) in shipped {
        languages.override_grammar(&name, GrammarSource::Local(sources));
    }
}

/// Whether the test sources of a language are on disk; `false` means `scripts/test-grammars`
/// has not fetched it yet.
pub fn has_test_grammar(language: &Language) -> bool {
    test_grammar_sources(language).is_dir()
}
