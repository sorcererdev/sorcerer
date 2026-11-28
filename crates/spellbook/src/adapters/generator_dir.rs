//! Generators on disk: each directory under `.sorcerer/spellbook/generators/` holds a
//! `generator.toml`, its `templates/` and, when an edit copies files unchanged, its `files/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sorcerer_core::domain::language::Languages;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;

use crate::domain::generator::{FILES_DIR, Generator, read_generator};
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
    let generator = load_alone(dir, languages)?;
    let generators_dir = dir.parent().unwrap_or(dir);
    uses_agree(
        generators_dir,
        languages,
        &generator,
        std::slice::from_ref(&generator.id),
    )
    .map_err(|why| GeneratorProblem {
        generator: generator.id.clone(),
        file: "generator.toml".into(),
        why,
    })?;
    Ok(generator)
}

/// How deep `[[uses]]` may go: a generator, the ones it uses, and so on, four in a chain.
const USES_DEPTH: usize = 4;

/// Each `[[uses]]` names a generator of the project that loads, gives only inputs it takes, and
/// leads to no cycle and no chain longer than four. `chain` is the generators that led here.
fn uses_agree(
    generators_dir: &Path,
    languages: &Languages,
    generator: &Generator,
    chain: &[String],
) -> Result<(), String> {
    for (number, used) in generator.uses.iter().enumerate() {
        let place = format!("[[uses]] {} of {}", number + 1, generator.id);
        if chain.contains(&used.generator) {
            return Err(format!(
                "{place}: a cycle ({} → {})",
                chain.join(" → "),
                used.generator
            ));
        }
        if chain.len() >= USES_DEPTH {
            return Err(format!(
                "{place}: [[uses]] go deeper than {USES_DEPTH} generators ({} → {})",
                chain.join(" → "),
                used.generator
            ));
        }
        let dir = generators_dir.join(&used.generator);
        if !dir.join("generator.toml").is_file() {
            return Err(format!(
                "{place}: the project has no generator `{}`",
                used.generator
            ));
        }
        let loaded =
            load_alone(&dir, languages).map_err(|problem| format!("{place}: {problem}"))?;
        if let Some(name) = used
            .with
            .keys()
            .find(|name| !loaded.inputs.iter().any(|(input, _)| input == *name))
        {
            return Err(format!(
                "{place}: `with` gives `{name}`, an input {} does not have",
                used.generator
            ));
        }
        let longer: Vec<String> = chain.iter().cloned().chain([loaded.id.clone()]).collect();
        uses_agree(generators_dir, languages, &loaded, &longer)?;
    }
    Ok(())
}

/// A generator read and checked against itself, its `[[uses]]` not followed.
fn load_alone(dir: &Path, languages: &Languages) -> Result<Generator, GeneratorProblem> {
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
    let files = files_in(&dir.join(FILES_DIR)).map_err(|why| problem("files/", why))?;
    let generator = Generator {
        files,
        ..read_generator(&id, &text, templates).map_err(|why| problem("generator.toml", why))?
    };
    check_generator(&generator, languages).map_err(|found| problem(&found.file, found.why))?;
    Ok(generator)
}

/// Every file under `templates/`, by its path there with `/`, and its text.
fn templates_in(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    files_in(dir)?
        .into_iter()
        .map(|(name, bytes)| match String::from_utf8(bytes) {
            Ok(text) => Ok((name, text)),
            Err(_) => Err(format!(
                "{name} is not UTF-8 text; a file to copy unchanged belongs in {FILES_DIR}/"
            )),
        })
        .collect()
}

/// Every file under a folder (none when it is not there), by its path there with `/`, and its
/// bytes.
fn files_in(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut found = BTreeMap::new();
    collect_files(dir, dir, &mut found)?;
    Ok(found)
}

fn collect_files(
    top: &Path,
    dir: &Path,
    found: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_files(top, &path, found)?;
        } else {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let name = path
                .strip_prefix(top)
                .map(forward_slashes)
                .unwrap_or_default();
            found.insert(name, bytes);
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
