//! The project's owned regions, read from `.sorcerer/spellbook/regions.toml`.

use sorcerer_core::domain::project::Project;

use crate::domain::region::{REGIONS_FILE, Region, RegionsProblem, parse_regions};

/// The project's regions, in file order; none when the file is not there.
///
/// Example: a project with no `regions.toml` has no regions; one whose region has `quary = …`
/// gets ".sorcerer/spellbook/regions.toml: region modules: `quary` is not a key; did you mean
/// `query`?".
pub fn load_regions(project: &Project) -> Result<Vec<Region>, RegionsProblem> {
    let path = project.root.join(REGIONS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_regions(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(RegionsProblem {
            why: format!("cannot be read ({e})"),
        }),
    }
}
