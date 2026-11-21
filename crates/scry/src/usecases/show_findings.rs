//! The findings an editor shows in one file, each on its line as the file is now.

use std::path::Path;

use sorcerer_core::ports::view::CodebaseView;

use crate::domain::finding::Finding;
use crate::domain::placing::{Placed, placed};
use crate::ports::finding_store::FindingStore;
use crate::ports::settings::Settings;
use crate::usecases::read_findings;

/// A finding to show, and where it sits now.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub finding: Finding,
    pub placed: Placed,
}

/// What scry shows in a file (its path relative to the project root), each placed in the file
/// as it is now:
///
/// 1. nothing when `[scry] enabled = false`;
/// 2. else the findings `scry` itself shows there (open, of the issue types the project
///    shows, the most confident first, at most 3), read from the store afresh;
/// 3. each placed by its unit now; a finding whose unit is gone is left out. A file whose
///    units cannot be read has none, so only its file-level findings are placed.
///
/// No other filter: the editor and `scry list` agree about what is open.
pub fn shown_in_file(
    store: &dyn FindingStore,
    settings: &dyn Settings,
    view: &dyn CodebaseView,
    file: &Path,
) -> anyhow::Result<Vec<Shown>> {
    let config = settings.scry_config()?;
    if !config.enabled {
        return Ok(Vec::new());
    }
    let stored = read_findings(store)?;
    let units_now = view.units(file).unwrap_or_default();
    let line_count = view.text(file).map_or(0, |text| line_count_of(&text));
    Ok(stored
        .shown_in(&config, file)
        .into_iter()
        .filter_map(|finding| {
            placed(finding, &units_now, line_count).map(|placed| Shown {
                finding: finding.clone(),
                placed,
            })
        })
        .collect())
}

/// The number of lines in a text; a last line without a newline counts.
fn line_count_of(text: &str) -> u32 {
    u32::try_from(text.lines().count()).unwrap_or(u32::MAX)
}
