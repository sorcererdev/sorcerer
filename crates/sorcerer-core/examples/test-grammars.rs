//! Fetches the shipped grammars the tests use, so the tests themselves never touch the network:
//!
//! ```text
//! scripts/test-grammars            # the same as:
//! cargo run -p sorcerer-core --example test-grammars -- target/test-grammars
//! ```
//!
//! For each shipped language whose grammar is not vendored under
//! `crates/sorcerer-core/tests/grammars/<name>/` (Python and Rust are), it fetches the pinned
//! commit into `<dir>/src/<name>-<first 7 of rev>/`, the grammar cache's own layout, with the
//! same checks a build makes (a commit hash, git's own transports, the commit that arrived).
//! A checkout already there is kept. It prints one line per language and exits 1 naming the
//! first that could not be fetched. The tests find the checkouts through
//! `tests/support/test_grammars.rs`.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use sorcerer_core::adapters::grammar::{GrammarCache, fetch_checkout};
use sorcerer_core::domain::language::{GrammarSource, Languages};

fn main() -> anyhow::Result<()> {
    let Some(out) = std::env::args().nth(1).map(PathBuf::from) else {
        bail!("usage: test-grammars OUT_DIR");
    };
    let nowhere = tempfile::TempDir::new()?;
    let shipped = Languages::assemble(nowhere.path(), &[])?;
    let cache = GrammarCache::at(out);
    for name in shipped.names() {
        let language = shipped.get(name).context("a shipped language")?;
        println!("{}", fetched_line(&cache, name, &language.grammar)?);
    }
    Ok(())
}

/// Fetches one language's grammar unless it is vendored, and says which it was:
/// `python: vendored`, `html: target/test-grammars/src/html-5a5ca85`.
fn fetched_line(
    cache: &GrammarCache,
    name: &str,
    grammar: &GrammarSource,
) -> anyhow::Result<String> {
    if vendored(name).is_dir() {
        return Ok(format!("{name}: vendored"));
    }
    let GrammarSource::Git { url, rev, .. } = grammar else {
        bail!("{name}: a shipped grammar is {{ git, rev }}");
    };
    let checkout =
        fetch_checkout(cache, name, url, rev).with_context(|| format!("grammar {name}"))?;
    Ok(format!("{name}: {}", checkout.display()))
}

fn vendored(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/grammars")
        .join(name)
}
