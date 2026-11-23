//! The place in the code a change is asked at.

use std::path::PathBuf;

use sorcerer_core::domain::facts::Facts;
use sorcerer_core::domain::language::Language;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::{Span, Unit};

use super::request_and_result::{ChangeDeps, ChangeRefused, ChangeRequest};

/// The place in the code a change is asked at.
pub(super) struct Site {
    pub(super) file: PathBuf,
    pub(super) language: Language,
    /// The file's text as the view has it (the editor's when open).
    pub(super) text: String,
    pub(super) unit: Unit,
    /// The unit's lines, or the person's selection.
    pub(super) span: Span,
    pub(super) facts: Facts,
}

/// The unit under the cursor (or at the start of the selection) and its facts. When the parser
/// lost the unit, the broken region around the cursor stands in for it, for the writer to repair.
/// A file of no language, or a line in no unit and no broken region, is `OutsideAUnit`.
pub(super) fn find_site(request: &ChangeRequest, deps: &ChangeDeps) -> Result<Site, ChangeRefused> {
    let failed = |e: sorcerer_core::Error| ChangeRefused::Failed(e.to_string());
    if !deps.fences.allows(&request.file) {
        return Err(ChangeRefused::Failed(format!(
            "{} is fenced: a model is never shown it",
            forward_slashes(&request.file)
        )));
    }
    let language = deps
        .project
        .languages
        .for_path(&request.file)
        .ok_or(ChangeRefused::OutsideAUnit)?;
    let text = deps.view.text(&request.file).map_err(failed)?;
    let anchor = request.selection.map_or(request.line, |lines| lines.start);
    let unit = deps
        .view
        .unit_or_broken_region_at(&request.file, anchor)
        .map_err(failed)?
        .ok_or(ChangeRefused::OutsideAUnit)?;
    let facts = deps.view.facts(&request.file, &unit).map_err(failed)?;
    Ok(Site {
        file: request.file.clone(),
        span: request.selection.unwrap_or(unit.span),
        language: language.clone(),
        text,
        unit,
        facts,
    })
}
