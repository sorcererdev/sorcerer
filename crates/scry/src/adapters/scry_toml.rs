//! Scry's settings from the project: `[scry]` in `.sorcerer/config.toml`
//! (already parsed with the project) and the steering in
//! `.sorcerer/scry.md`.

use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::config::{ScryConfig, scry_config};
use crate::domain::steering::{SHIPPED, Steering, steering_of};
use crate::ports::settings::Settings;

pub struct ScryToml {
    project: Project,
}

impl ScryToml {
    pub fn for_project(project: &Project) -> ScryToml {
        ScryToml {
            project: project.clone(),
        }
    }
}

impl Settings for ScryToml {
    fn scry_config(&self) -> Result<ScryConfig> {
        scry_config(&self.project)
    }

    /// The shipped layer, then `.sorcerer/scry.md` when the project has one.
    fn steering(&self) -> Result<Steering> {
        let file = self.project.root.join(".sorcerer/scry.md");
        match std::fs::read_to_string(&file) {
            Ok(t) => Ok(steering_of(&[SHIPPED, &t])),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(steering_of(&[SHIPPED])),
            Err(e) => Err(Error::io(&file, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "## Rules\n\n### Commands return a Result (must)\nA command never panics.\n\n## Other\n\n### Not a rule\nx\n";

    #[test]
    fn the_project_file_is_a_second_layer() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".sorcerer")).unwrap();
        std::fs::write(d.path().join(".sorcerer/scry.md"), SAMPLE).unwrap();
        let p = Project::open(d.path()).unwrap();
        assert_eq!(ScryToml::for_project(&p).steering().unwrap().rules.len(), 1);
    }
}
