//! `[scry]` in `.sorcerer/config.toml`, read strictly and range-checked.

use serde::Deserialize;
use sorcerer_core::{Error, Project, Result};

use crate::kind::kind;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ScryConfig {
    pub enabled: bool,
    pub kinds: Vec<String>,
    pub find_model: String,
    pub cut: f64,
    pub max_shown: usize,
    pub settle: f64,
    pub pass_cost: f64,
    pub day_cost: f64,
    pub exclude: Vec<String>,
}

impl Default for ScryConfig {
    fn default() -> ScryConfig {
        ScryConfig {
            enabled: true,
            kinds: ["bug", "assumption", "convention"]
                .map(String::from)
                .to_vec(),
            find_model: "sonnet".into(),
            cut: 0.7,
            max_shown: 3,
            settle: 2.0,
            pass_cost: 0.25,
            day_cost: 3.00,
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
    if !(0.0..=1.0).contains(&c.cut) {
        return Err(bad(format!("cut = {} is outside 0..=1", c.cut)));
    }
    if c.max_shown < 1 {
        return Err(bad("max_shown must be at least 1".into()));
    }
    for (key, v) in [
        ("settle", c.settle),
        ("pass_cost", c.pass_cost),
        ("day_cost", c.day_cost),
    ] {
        if v.is_nan() || v < 0.0 {
            return Err(bad(format!("{key} = {v} must not be negative")));
        }
    }
    if let Some(k) = c.kinds.iter().find(|k| kind(k).is_none()) {
        return Err(bad(format!(
            "kinds: `{k}` is not a kind (bug, assumption, convention, design)"
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
        assert_eq!(c.kinds, ["bug", "assumption", "convention"]);
        assert_eq!(c.find_model, "sonnet");
        assert_eq!((c.cut, c.max_shown, c.settle), (0.7, 3, 2.0));
        assert_eq!((c.pass_cost, c.day_cost), (0.25, 3.00));
        assert!(c.exclude.is_empty());
    }

    #[test]
    fn a_table_overrides_and_keeps_the_rest() {
        let (_d, p) = project(Some("[scry]\nmax_shown = 5\nexclude = [\"gen/**\"]\n"));
        let c = scry_config(&p).unwrap();
        assert_eq!(c.max_shown, 5);
        assert_eq!(c.exclude, ["gen/**"]);
        assert_eq!(c.cut, 0.7);
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let (_d, p) = project(Some("[scry]\ncutt = 0.5\n"));
        let e = scry_config(&p).unwrap_err().to_string();
        assert!(e.contains("cutt"), "{e}");
    }

    #[test]
    fn out_of_range_values_are_refused() {
        for (toml, key) in [
            ("cut = 1.5", "cut"),
            ("max_shown = 0", "max_shown"),
            ("kinds = [\"style\"]", "style"),
            ("settle = -1.0", "settle"),
            ("pass_cost = -0.1", "pass_cost"),
            ("day_cost = -3.0", "day_cost"),
        ] {
            let (_d, p) = project(Some(&format!("[scry]\n{toml}\n")));
            let e = scry_config(&p).unwrap_err().to_string();
            assert!(e.contains(key), "{toml}: {e}");
            assert!(e.contains("config.toml"), "{toml}: {e}");
        }
    }
}
