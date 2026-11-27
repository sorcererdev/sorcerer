//! The project's baseline, read from and written to `.sorcerer/spellbook/baseline.toml`.

use sorcerer_core::domain::project::Project;

use crate::domain::baseline::{Baseline, baseline_text, parse_baseline};

/// Where the baseline is kept, relative to the project root. It is the team's, so it is
/// committed beside `rules.toml`.
pub const BASELINE_FILE: &str = ".sorcerer/spellbook/baseline.toml";

/// The project's baseline; an empty one when the file is not there. The error is the sentence
/// to show, naming the file.
///
/// Example: a `baseline.toml` with `line = 3` in an entry gives
/// ``.sorcerer/spellbook/baseline.toml: unknown field `line`, …``.
pub fn load_baseline(project: &Project) -> Result<Baseline, String> {
    let path = project.root.join(BASELINE_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_baseline(&text).map_err(|why| format!("{BASELINE_FILE}: {why}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Baseline::default()),
        Err(e) => Err(format!("{BASELINE_FILE}: cannot be read ({e})")),
    }
}

/// Writes the whole baseline file, replacing what was there.
pub fn write_baseline(project: &Project, baseline: &Baseline) -> Result<(), String> {
    let path = project.root.join(BASELINE_FILE);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{BASELINE_FILE}: {e}"))?;
    }
    std::fs::write(&path, baseline_text(baseline)).map_err(|e| format!("{BASELINE_FILE}: {e}"))
}
