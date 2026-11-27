//! lackey's settings: the `[lackey]` table of `.sorcerer/config.toml`. An unknown key,
//! or a value out of range, is an error naming the key.

use serde::{Deserialize, Serialize};
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

/// The product lackey's errands are recorded under.
pub const PRODUCT: &str = "lackey";

/// The `[lackey]` table, every key with its default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LackeyConfig {
    /// The model a task runs on: `sonnet` by default.
    pub model: String,
    /// The most one task may spend, in dollars: 2.00 by default (passed to `claude` as
    /// `--max-budget-usd`, which may overshoot by one call).
    pub budget_per_task: f64,
    /// The most lackey may spend in a local day, in dollars: 5.00 by default. When it is
    /// spent, offers say so and nothing runs unasked.
    pub budget_per_day: f64,
    /// The most Jev's triage may spend in a local day, in dollars: 0.20 by default.
    pub triage_per_day: f64,
    /// The most turns a task may take: 60 by default.
    pub max_turns: u32,
    /// How long a question to the person waits before the agent carries on with its default, in
    /// seconds: 120 by default.
    pub ask_timeout_seconds: u64,
    /// The estimate an offer shows before five tasks have finished, in dollars: 0.50.
    pub estimate_start: f64,
}

impl Default for LackeyConfig {
    fn default() -> LackeyConfig {
        LackeyConfig {
            model: "sonnet".into(),
            budget_per_task: 2.00,
            budget_per_day: 5.00,
            triage_per_day: 0.20,
            max_turns: 60,
            ask_timeout_seconds: 120,
            estimate_start: 0.50,
        }
    }
}

/// The project's lackey settings: the `[lackey]` table, with the defaults for keys it
/// leaves out. Every amount must be a finite number that is not negative, the model must not be
/// empty and a task needs at least one turn; otherwise the error names the key and the file.
///
/// Example: `budget_per_task = -1` is refused with
/// `[lackey] budget_per_task = -1 must not be negative`.
pub fn lackey_config(project: &Project) -> Result<LackeyConfig> {
    let config: LackeyConfig = project.config.table("lackey")?;
    match config.first_problem() {
        None => Ok(config),
        Some(why) => Err(Error::Config {
            file: project.root.join(".sorcerer/config.toml"),
            why: format!("[lackey] {why}"),
        }),
    }
}

impl LackeyConfig {
    /// The first value out of range, as the error message tells it.
    pub fn first_problem(&self) -> Option<String> {
        let amounts = [
            ("budget_per_task", self.budget_per_task),
            ("budget_per_day", self.budget_per_day),
            ("triage_per_day", self.triage_per_day),
            ("estimate_start", self.estimate_start),
        ];
        amounts
            .iter()
            .find_map(|(key, dollars)| amount_problem(key, *dollars))
            .or_else(|| {
                self.model
                    .is_empty()
                    .then(|| "model is empty; name a model alias such as \"sonnet\"".to_string())
            })
            .or_else(|| {
                (self.max_turns == 0).then(|| "max_turns = 0 must be at least 1".to_string())
            })
    }
}

fn amount_problem(key: &str, dollars: f64) -> Option<String> {
    if !dollars.is_finite() {
        return Some(format!("{key} = {dollars} must be a finite number"));
    }
    (dollars < 0.0).then(|| format!("{key} = {dollars} must not be negative"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_rulings_caps() {
        let config = LackeyConfig::default();
        assert_eq!(
            (
                config.budget_per_task,
                config.budget_per_day,
                config.triage_per_day
            ),
            (2.00, 5.00, 0.20)
        );
        assert_eq!(config.first_problem(), None);
    }

    #[test]
    fn a_value_out_of_range_is_named() {
        let config = LackeyConfig {
            budget_per_task: -1.0,
            ..LackeyConfig::default()
        };
        assert_eq!(
            config.first_problem().unwrap(),
            "budget_per_task = -1 must not be negative"
        );
        let config = LackeyConfig {
            max_turns: 0,
            ..LackeyConfig::default()
        };
        assert_eq!(
            config.first_problem().unwrap(),
            "max_turns = 0 must be at least 1"
        );
    }
}
