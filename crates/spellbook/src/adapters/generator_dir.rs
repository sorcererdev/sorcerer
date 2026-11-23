//! Generators on disk: each directory under `.sorcerer/spellbook/generators/` holds a
//! `generator.toml` and its `templates/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sorcerer_core::domain::language::Languages;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;

use crate::domain::generator::{Generator, read_generator};
use crate::domain::generator_checks::check_generator;
use crate::domain::generator_problems::GeneratorProblem;
use crate::ports::generators::Generators;

/// Where a project keeps its generators, relative to its root.
pub const GENERATORS_DIR: &str = ".sorcerer/spellbook/generators";

/// Every generator of the project, by directory name, each loaded or the problem that stopped
/// it: one broken generator does not hide the others.
pub fn generators_in(project: &Project) -> Vec<Result<Generator, GeneratorProblem>> {
    generator_dirs(&project.root)
        .iter()
        .map(|dir| load_generator(dir, &project.languages))
        .collect()
}

/// The generator with this name, or the problem that stopped it; `None` when the project has no
/// such directory.
pub fn generator_named(project: &Project, id: &str) -> Option<Result<Generator, GeneratorProblem>> {
    let plain = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !plain {
        return None;
    }
    let dir = project.root.join(GENERATORS_DIR).join(id);
    dir.join("generator.toml")
        .is_file()
        .then(|| load_generator(&dir, &project.languages))
}

/// The directories under the generators folder that hold a `generator.toml`, by name.
pub fn generator_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join(GENERATORS_DIR)) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|dir| dir.join("generator.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

/// One generator from its directory: the file read, its templates read, and every check run.
pub fn load_generator(dir: &Path, languages: &Languages) -> Result<Generator, GeneratorProblem> {
    let id = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let problem = |file: &str, why: String| GeneratorProblem {
        generator: id.clone(),
        file: file.to_string(),
        why,
    };
    let text = std::fs::read_to_string(dir.join("generator.toml"))
        .map_err(|e| problem("generator.toml", e.to_string()))?;
    let templates =
        templates_in(&dir.join("templates")).map_err(|why| problem("templates/", why))?;
    let generator =
        read_generator(&id, &text, templates).map_err(|why| problem("generator.toml", why))?;
    check_generator(&generator, languages).map_err(|found| problem(&found.file, found.why))?;
    Ok(generator)
}

/// Every file under `templates/`, by its path there with `/`, and its text.
fn templates_in(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut found = BTreeMap::new();
    collect_templates(dir, dir, &mut found)?;
    Ok(found)
}

fn collect_templates(
    top: &Path,
    dir: &Path,
    found: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_templates(top, &path, found)?;
        } else {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let name = path
                .strip_prefix(top)
                .map(forward_slashes)
                .unwrap_or_default();
            found.insert(name, text);
        }
    }
    Ok(())
}

/// The project's generator directories, as the use cases ask for them.
pub struct GeneratorDirs {
    project: Arc<Project>,
}

impl GeneratorDirs {
    pub fn new(project: Arc<Project>) -> GeneratorDirs {
        GeneratorDirs { project }
    }
}

impl Generators for GeneratorDirs {
    fn all(&self) -> Vec<Result<Generator, GeneratorProblem>> {
        generators_in(&self.project)
    }

    fn named(&self, id: &str) -> Option<Result<Generator, GeneratorProblem>> {
        generator_named(&self.project, id)
    }
}
