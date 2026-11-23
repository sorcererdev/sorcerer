//! `scry why`: one finding in full, as Markdown: what it says, the rule
//! and the unit, the code it cites, what the check said, and its history.

use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use chrono::FixedOffset;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::scry_toml::ScryToml;
use crate::adapters::why_text::why_text;
use crate::usecases::explain_finding::explain_finding;

/// Prints the finding with this id (or the start of one, if only one
/// finding has it) in full.
pub(super) fn why(
    project: &Project,
    env: &dyn Env,
    id: &str,
    offset: FixedOffset,
    out: &mut dyn Write,
) -> Result<i32> {
    let view = FilesView::new(Arc::new(project.clone()), GrammarCache::user(env));
    let explanation = explain_finding(
        project,
        &JsonFindingStore::for_project(project)?,
        &ScryToml::for_project(project),
        &view,
        id,
    )?;
    write!(out, "{}", why_text(&explanation, offset))?;
    Ok(0)
}
