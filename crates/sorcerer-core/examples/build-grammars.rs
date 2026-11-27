//! Builds the prebuilt grammar files a release carries, for the platform it runs on:
//!
//! ```text
//! cargo run -p sorcerer-core --example build-grammars -- grammars
//! ```
//!
//! For each language sorcerer ships, it fetches the grammar's pinned sources and compiles them
//! the way scry would, into a cache at the directory given, so each library has the very name
//! scry asks the release for (`<name>-<key>.<dylib|so|dll>`). Beside each it writes
//! `<file>.sha256` (`<64 hex digits>  <file name>`), then removes the fetched sources, and
//! prints each library's file name and size. The release job runs it once on each platform's
//! runner. It exits 1 naming the language when one fails to build or is larger than scry
//! downloads (`LIBRARY_CAP`, 20 MB): such a release would send every user to a C compiler.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use sorcerer_core::adapters::env::SystemEnv;
use sorcerer_core::adapters::grammar::{GrammarCache, LIBRARY_CAP, build_if_absent, sha256_hex};
use sorcerer_core::adapters::platform::compiler::c_compiler;
use sorcerer_core::domain::language::Languages;

fn main() -> anyhow::Result<()> {
    let Some(out) = std::env::args().nth(1).map(PathBuf::from) else {
        bail!("usage: build-grammars OUT_DIR");
    };
    let shipped = shipped_languages()?;
    for name in shipped.names() {
        let library = build_one(&shipped, name, &out).with_context(|| format!("grammar {name}"))?;
        let size = size_under_the_cap(&library).with_context(|| format!("grammar {name}"))?;
        write_sum_beside(&library)?;
        println!("{}  {}", file_name(&library), size_text(size));
    }
    remove_fetched_sources(&out)
}

/// The languages sorcerer ships, with no project laid on top.
fn shipped_languages() -> anyhow::Result<Languages> {
    let nowhere = tempfile::TempDir::new()?;
    Ok(Languages::assemble(nowhere.path(), &[])?)
}

/// Fetches and compiles one shipped grammar into the cache at `out`; the library's path.
fn build_one(shipped: &Languages, name: &str, out: &Path) -> anyhow::Result<PathBuf> {
    let language = shipped.get(name).context("not a shipped language")?;
    let compiler = c_compiler(&SystemEnv);
    let cache = GrammarCache::at(out.to_path_buf());
    Ok(build_if_absent(
        &cache,
        name,
        &language.grammar,
        compiler.as_ref(),
    )?)
}

/// The library's size in bytes, or an error when scry would refuse to download it.
fn size_under_the_cap(library: &Path) -> anyhow::Result<u64> {
    let size = std::fs::metadata(library)?.len();
    if size > LIBRARY_CAP {
        bail!(
            "{} is {}, over the {} scry downloads",
            file_name(library),
            size_text(size),
            size_text(LIBRARY_CAP)
        );
    }
    Ok(size)
}

/// A size as the release job's log shows it: `13.4 MB`, or `52 KB` under a megabyte.
fn size_text(bytes: u64) -> String {
    let megabytes = bytes as f64 / (1024.0 * 1024.0);
    if megabytes < 1.0 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{megabytes:.1} MB")
    }
}

/// Writes `<library>.sha256`, the way the release's other checksum files read.
fn write_sum_beside(library: &Path) -> anyhow::Result<()> {
    let bytes = std::fs::read(library)?;
    let line = format!("{}  {}\n", sha256_hex(&bytes), file_name(library));
    let mut sum_path = library.as_os_str().to_os_string();
    sum_path.push(".sha256");
    std::fs::write(&sum_path, line)?;
    Ok(())
}

/// The fetched sources are not part of the release.
fn remove_fetched_sources(out: &Path) -> anyhow::Result<()> {
    let sources = out.join("src");
    if sources.exists() {
        std::fs::remove_dir_all(&sources)?;
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
