//! conjure's settings: the `[conjure]` table of `.sorcerer/config.toml`. An unknown key, or a
//! value out of range, is an error naming the key.

use globset::Glob;
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

/// The product conjure's errands are recorded under.
pub const PRODUCT: &str = "conjure";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ConjureConfig {
    /// The model alias that writes the first attempt: `haiku` by default.
    pub writer: String,
    /// The stronger model Try harder asks: `sonnet` by default; empty turns Try harder off.
    pub try_harder: String,
    /// The most conjure may spend in a day, in dollars: 3.00 by default.
    pub budget_per_day: f64,
    /// Glob patterns of files conjure never writes, such as `gen/**`.
    pub exclude: Vec<String>,
}

impl Default for ConjureConfig {
    fn default() -> ConjureConfig {
        ConjureConfig {
            writer: "haiku".into(),
            try_harder: "sonnet".into(),
            budget_per_day: 3.00,
            exclude: Vec::new(),
        }
    }
}

/// The project's conjure settings: the `[conjure]` table, with the defaults for keys it leaves
/// out. The budget must be a number that is not negative, the writer must not be empty, and
/// every `exclude` pattern must be a glob; otherwise the error names the key and the config file.
///
/// Example: `budget_per_day = -1` is refused with
/// `[conjure] budget_per_day = -1 must not be negative`.
pub fn conjure_config(project: &Project) -> Result<ConjureConfig> {
    let config: ConjureConfig = project.config.table("conjure")?;
    match config.first_problem() {
        None => Ok(config),
        Some(why) => Err(Error::Config {
            file: project.root.join(".sorcerer/config.toml"),
            why: format!("[conjure] {why}"),
        }),
    }
}

impl ConjureConfig {
    /// Whether the mark offers Try harder: it is off when `try_harder` is empty.
    pub fn offers_try_harder(&self) -> bool {
        !self.try_harder.is_empty()
    }

    /// The first value out of range, as the error message tells it.
    pub fn first_problem(&self) -> Option<String> {
        self.budget_problem()
            .or_else(|| self.writer_problem())
            .or_else(|| self.exclude_problem())
    }

    fn budget_problem(&self) -> Option<String> {
        let dollars = self.budget_per_day;
        if !dollars.is_finite() {
            return Some(format!(
                "budget_per_day = {dollars} must be a finite number"
            ));
        }
        (dollars < 0.0).then(|| format!("budget_per_day = {dollars} must not be negative"))
    }

    fn writer_problem(&self) -> Option<String> {
        self.writer
            .is_empty()
            .then(|| "writer is empty; name a model alias such as \"haiku\"".to_string())
    }

    fn exclude_problem(&self) -> Option<String> {
        self.exclude.iter().find_map(|pattern| {
            Glob::new(pattern)
                .err()
                .map(|e| format!("exclude: `{pattern}` is not a glob ({e})"))
        })
    }
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
        let c = conjure_config(&p).unwrap();
        assert_eq!(c.writer, "haiku");
        assert_eq!(c.try_harder, "sonnet");
        assert_eq!(c.budget_per_day, 3.00);
        assert!(c.exclude.is_empty());
        assert!(c.offers_try_harder());
    }

    #[test]
    fn a_table_overrides_and_keeps_the_rest() {
        let (_d, p) = project(Some(
            "[conjure]\nbudget_per_day = 0.5\nexclude = [\"gen/**\"]\n",
        ));
        let c = conjure_config(&p).unwrap();
        assert_eq!(c.budget_per_day, 0.5);
        assert_eq!(c.exclude, ["gen/**"]);
        assert_eq!(c.writer, "haiku");
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let (_d, p) = project(Some("[conjure]\nwriterr = \"x\"\n"));
        let e = conjure_config(&p).unwrap_err().to_string();
        assert!(e.contains("writerr"), "{e}");
    }

    #[test]
    fn a_negative_budget_is_refused() {
        let (_d, p) = project(Some("[conjure]\nbudget_per_day = -1.0\n"));
        let e = conjure_config(&p).unwrap_err().to_string();
        assert!(e.contains("budget_per_day"), "{e}");
        assert!(e.contains("config.toml"), "{e}");
    }

    #[test]
    fn an_empty_writer_and_a_bad_glob_are_refused_by_name() {
        for (toml, key) in [
            ("writer = \"\"", "writer"),
            ("exclude = [\"[\"]", "exclude"),
        ] {
            let (_d, p) = project(Some(&format!("[conjure]\n{toml}\n")));
            let e = conjure_config(&p).unwrap_err().to_string();
            assert!(e.contains(key), "{toml}: {e}");
        }
    }

    #[test]
    fn a_budget_that_is_not_a_finite_number_is_refused() {
        for value in ["inf", "nan", "-inf"] {
            let (_d, p) = project(Some(&format!("[conjure]\nbudget_per_day = {value}\n")));
            let e = conjure_config(&p).unwrap_err().to_string();
            assert!(
                e.contains("budget_per_day") && e.contains("finite"),
                "{value}: {e}"
            );
        }
    }

    #[test]
    fn an_empty_try_harder_offers_no_try_harder() {
        let (_d, p) = project(Some("[conjure]\ntry_harder = \"\"\n"));
        assert!(!conjure_config(&p).unwrap().offers_try_harder());
    }
}
