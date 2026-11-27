//! Where lackey keeps its state: `.sorcerer/state/lackey/`, which core's state
//! directory keeps out of git.

use std::path::PathBuf;

use sorcerer_core::Result;
use sorcerer_core::domain::project::Project;

/// The files under `.sorcerer/state/lackey/`.
#[derive(Debug, Clone)]
pub struct StateFiles {
    pub dir: PathBuf,
}

impl StateFiles {
    /// The project's lackey state directory (made, with the state directory's
    /// `.gitignore`, when it is not there).
    pub fn for_project(project: &Project) -> Result<StateFiles> {
        Ok(StateFiles {
            dir: project.state_dir()?.join("lackey"),
        })
    }

    /// `tasks.jsonl`: one line per change of a task's state.
    pub fn tasks(&self) -> PathBuf {
        self.dir.join("tasks.jsonl")
    }

    /// `tasks/`: each task's journal and before-texts.
    pub fn task_dirs(&self) -> PathBuf {
        self.dir.join("tasks")
    }

    /// `notes/`: where notes were kept before they moved to `.sorcerer/notes/`
    /// ([`NotesDir`](crate::adapters::notes_dir::NotesDir)), so they can be moved on start.
    pub fn old_notes(&self) -> PathBuf {
        self.dir.join("notes")
    }

    /// `permissions.json`: the commands allowed always.
    pub fn permissions(&self) -> PathBuf {
        self.dir.join("permissions.json")
    }

    /// `regions.json`: each region Jev looked at, and its answer.
    pub fn regions(&self) -> PathBuf {
        self.dir.join("regions.json")
    }

    /// `labels.jsonl`: the person's labels of regions.
    pub fn labels(&self) -> PathBuf {
        self.dir.join("labels.jsonl")
    }
}
