//! Prints a file's units with their fingerprints, then how they changed
//! against HEAD. Run from inside a project:
//!
//! ```text
//! cargo run -p sorcerer-core --example units -- src/orders.py
//! ```
//!
//! The first run for a language fetches and compiles its grammar into
//! `~/.cache/sorcerer/grammars`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail};
use sorcerer_core::Project;
use sorcerer_core::compare::{UnitChange, compare_units};
use sorcerer_core::env::SystemEnv;
use sorcerer_core::grammar::GrammarCache;
use sorcerer_core::unit::units_of;
use sorcerer_core::view::{CodebaseView, FilesView};

fn main() -> anyhow::Result<()> {
    let Some(arg) = std::env::args().nth(1) else {
        bail!("usage: units PATH");
    };
    let project = Arc::new(Project::open(&std::env::current_dir()?)?);
    let path = std::env::current_dir()?
        .join(&arg)
        .canonicalize()
        .with_context(|| arg.clone())?;
    let rel: PathBuf = path
        .strip_prefix(&project.root)
        .context("the file is outside the project")?
        .into();
    let Some(lang) = project.languages.for_path(&rel) else {
        bail!("no language covers {}", rel.display());
    };
    let cache = GrammarCache::user(&SystemEnv);
    let view = FilesView::new(project.clone(), cache.clone());
    let now = view.units(&rel)?;
    for u in &now {
        println!(
            "{:>4}-{:<4}  {}  {:?}  {}",
            u.span.start, u.span.end, u.fingerprint, u.kind, u.name
        );
    }
    let before = match view.text_at(&rel, "HEAD")? {
        Some(text) => units_of(&*project.languages.load(&lang.name, &cache)?, &text)?,
        None => Vec::new(),
    };
    println!();
    for c in compare_units(&before, &now) {
        match c {
            UnitChange::Added(u) => println!("added    {}", u.name),
            UnitChange::Changed { after, .. } => println!("changed  {}", after.name),
            UnitChange::Removed(u) => println!("removed  {}", u.name),
            UnitChange::Same(_) => {}
        }
    }
    Ok(())
}
