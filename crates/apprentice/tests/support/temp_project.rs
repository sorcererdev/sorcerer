//! A project in a temporary directory, and the task deps over it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use apprentice::adapters::wiring::task_deps;
use apprentice::ports::agent::Agent;
use apprentice::ports::editor::Editor;
use apprentice::usecases::run_task::TaskDeps;
use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::domain::project::Project;
use tempfile::TempDir;

use super::FixedClock;

pub struct TempProject {
    pub dir: TempDir,
    pub root: PathBuf,
}

impl TempProject {
    /// A project with `.sorcerer/` and the files given.
    pub fn with(files: &[(&str, &str)]) -> TempProject {
        let dir = TempDir::new().unwrap();
        let root = sorcerer_core::adapters::platform::paths::canonical(dir.path()).unwrap();
        std::fs::create_dir_all(root.join(".sorcerer")).unwrap();
        for (path, text) in files {
            write(&root.join(path), text);
        }
        TempProject { dir, root }
    }

    pub fn project(&self) -> Project {
        Project::open(&self.root).unwrap()
    }

    pub fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.join(path)).unwrap()
    }

    pub fn write(&self, path: &str, text: &str) {
        write(&self.root.join(path), text);
    }

    /// The deps of a task over this project, with the editor and agent given.
    pub fn deps(&self, editor: Arc<dyn Editor>, agent: Arc<dyn Agent>) -> TaskDeps {
        let env = MapEnv::default();
        task_deps(
            &self.project(),
            &env,
            editor,
            agent,
            Arc::new(FixedClock::thanksgiving()),
        )
        .unwrap()
    }

    pub fn state(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.join(".sorcerer/state/apprentice").join(path))
            .unwrap_or_default()
    }
}

pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
