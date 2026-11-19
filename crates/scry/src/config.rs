//! `[scry]` in `.sorcerer/config.toml`, read strictly and range-checked.

use serde::{Deserialize, Serialize};
use sorcerer_core::{Error, Project, Result};

use crate::issue_type::issue_type;

/// At most this many findings are shown in a file at a time.
pub const MAX_SHOWN: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ScryConfig {
    pub enabled: bool,
    pub issue_types: Vec<String>,
    pub review_model: String,
    pub confidence_threshold: f64,
    pub budget_per_pass: f64,
    pub budget_per_day: f64,
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

/// The project's `[scry]` table; an error names the key and the config file.
pub fn scry_config(p: &Project) -> Result<ScryConfig> {
    let c: ScryConfig = p.config.table("scry")?;
    let file = p.root.join(".sorcerer/config.toml");
    let bad = |why: String| Error::Config {
        file: file.clone(),
        why: format!("[scry] {why}"),
    };
    if !(0.0..=1.0).contains(&c.confidence_threshold) {
        return Err(bad(format!(
            "confidence_threshold = {} is outside 0..=1",
            c.confidence_threshold
        )));
    }
    for (key, v) in [
        ("budget_per_pass", c.budget_per_pass),
        ("budget_per_day", c.budget_per_day),
    ] {
        if v.is_nan() || v < 0.0 {
            return Err(bad(format!("{key} = {v} must not be negative")));
        }
    }
    if let Some(k) = c.issue_types.iter().find(|k| issue_type(k).is_none()) {
        return Err(bad(format!(
            "issue_types: `{k}` is not an issue type (bug, assumption, convention, design)"
        )));
    }
    Ok(c)
}

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
