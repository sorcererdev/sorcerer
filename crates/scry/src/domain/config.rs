//! scry's settings: the `[scry]` table of `.sorcerer/config.toml`. An
//! unknown key, or a value out of range, is an error naming the key.

use serde::{Deserialize, Serialize};
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::issue_type::issue_type;

/// At most 3 findings are shown in a file at a time.
pub const MAX_SHOWN: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ScryConfig {
    /// Whether scry reviews at all. On by default.
    pub enabled: bool,
    /// The issue types shown to the user. By default `bug`, `assumption`
    /// and `convention`; `design` is raised but kept quiet.
    pub issue_types: Vec<String>,
    /// The model that reviews the code: `sonnet` by default.
    pub review_model: String,
    /// How sure Jev must be that a finding is worth saying before it is
    /// shown, from 0 to 1: 0.7 by default.
    pub confidence_threshold: f64,
    /// The most one pass may spend, in dollars: 0.25 by default.
    pub budget_per_pass: f64,
    /// The most all passes may spend in a day, in dollars: 3.00 by default.
    pub budget_per_day: f64,
    /// Glob patterns of files never reviewed, such as `gen/**`.
    pub exclude: Vec<String>,
}

impl Default for ScryConfig {
    fn default() -> ScryConfig {
        ScryConfig {
            enabled: true,
            issue_types: ["bug", "assumption", "convention"]
                .map(String::from)
                .to_vec(),
            review_model: "sonnet".into(),
            confidence_threshold: 0.7,
            budget_per_pass: 0.25,
            budget_per_day: 3.00,
            exclude: Vec::new(),
        }
    }
}

/// The project's scry settings: the `[scry]` table, with the defaults for
/// keys it leaves out. The confidence threshold must be from 0 to 1, the
/// budgets must not be negative, and the issue types must be known ones;
/// otherwise the error names the key and the config file.
///
/// Example: `confidence_threshold = 1.5` is refused with
/// `[scry] confidence_threshold = 1.5 is outside 0..=1`.
pub fn scry_config(project: &Project) -> Result<ScryConfig> {
    let config: ScryConfig = project.config.table("scry")?;
    match config.first_problem() {
        None => Ok(config),
        Some(why) => Err(Error::Config {
            file: project.root.join(".sorcerer/config.toml"),
            why: format!("[scry] {why}"),
        }),
    }
}

impl ScryConfig {
    /// The first value out of range, as the error message tells it.
    fn first_problem(&self) -> Option<String> {
        self.threshold_problem()
            .or_else(|| self.budget_problem())
            .or_else(|| self.issue_type_problem())
    }

    fn threshold_problem(&self) -> Option<String> {
        let threshold = self.confidence_threshold;
        (!(0.0..=1.0).contains(&threshold))
            .then(|| format!("confidence_threshold = {threshold} is outside 0..=1"))
    }

    /// A budget below 0, or not a number at all.
    fn budget_problem(&self) -> Option<String> {
        [
            ("budget_per_pass", self.budget_per_pass),
            ("budget_per_day", self.budget_per_day),
        ]
        .into_iter()
        .find(|(_, dollars)| dollars.is_nan() || *dollars < 0.0)
        .map(|(key, dollars)| format!("{key} = {dollars} must not be negative"))
    }

    fn issue_type_problem(&self) -> Option<String> {
        let unknown = self
            .issue_types
            .iter()
            .find(|name| issue_type(name).is_none())?;
        Some(format!(
            "issue_types: `{unknown}` is not an issue type (bug, assumption, convention, design)"
        ))
    }
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;

    fn project(toml: Option<&str>) -> (tempfile::TempDir, Project) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".sorcerer")).unwrap();
        if let Some(t) = toml {
            std::fs::write(d.path().join(".sorcerer/config.toml"), t).unwrap();
        }
        let p = Project::open(d.path()).unwrap();
        (d, p)
    }

    #[test]
    fn defaults_with_no_table() {
        let (_d, p) = project(None);
        let c = scry_config(&p).unwrap();
        assert!(c.enabled);
        assert_eq!(c.issue_types, ["bug", "assumption", "convention"]);
        assert_eq!(c.review_model, "sonnet");
        assert_eq!(c.confidence_threshold, 0.7);
        assert_eq!((c.budget_per_pass, c.budget_per_day), (0.25, 3.00));
        assert!(c.exclude.is_empty());
        assert_eq!(MAX_SHOWN, 3);
    }

    #[test]
    fn a_table_overrides_and_keeps_the_rest() {
        let (_d, p) = project(Some(
            "[scry]\nbudget_per_pass = 0.5\nexclude = [\"gen/**\"]\n",
        ));
        let c = scry_config(&p).unwrap();
        assert_eq!(c.budget_per_pass, 0.5);
        assert_eq!(c.exclude, ["gen/**"]);
        assert_eq!(c.confidence_threshold, 0.7);
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let (_d, p) = project(Some("[scry]\ncutt = 0.5\n"));
        let e = scry_config(&p).unwrap_err().to_string();
        assert!(e.contains("cutt"), "{e}");
    }

    #[test]
    fn an_old_key_is_refused_by_name() {
        for (toml, key) in [
            ("cut = 0.5", "cut"),
            ("max_shown = 3", "max_shown"),
            ("kinds = [\"bug\"]", "kinds"),
            ("find_model = \"haiku\"", "find_model"),
            ("pass_cost = 0.1", "pass_cost"),
            ("day_cost = 1.0", "day_cost"),
            ("settle = 2.0", "settle"),
        ] {
            let (_d, p) = project(Some(&format!("[scry]\n{toml}\n")));
            let e = scry_config(&p).unwrap_err().to_string();
            assert!(e.contains(key), "{toml}: {e}");
        }
    }

    #[test]
    fn out_of_range_values_are_refused() {
        for (toml, key) in [
            ("confidence_threshold = 1.5", "confidence_threshold"),
            ("issue_types = [\"style\"]", "style"),
            ("budget_per_pass = -0.1", "budget_per_pass"),
            ("budget_per_day = -3.0", "budget_per_day"),
            ("budget_per_day = nan", "budget_per_day"),
        ] {
            let (_d, p) = project(Some(&format!("[scry]\n{toml}\n")));
            let e = scry_config(&p).unwrap_err().to_string();
            assert!(e.contains(key), "{toml}: {e}");
            assert!(e.contains("config.toml"), "{toml}: {e}");
        }
    }
}
