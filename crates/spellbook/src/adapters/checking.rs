//! Everything checking needs for one project, put together: the rules read fresh, the settings,
//! and the runner over the project's grammars.

use crate::adapters::query_rules::QueryRules;
use crate::adapters::rules_toml::load_rules;
use crate::adapters::wiring::Wired;
use crate::domain::config::spellbook_config;
use crate::domain::rule::Rule;
use crate::usecases::check_files::CheckDeps;

pub struct Checking<'w> {
    wired: &'w Wired,
    pub rules: Vec<Rule>,
    exclude: Vec<String>,
    runner: QueryRules<'w>,
}

impl Wired {
    /// The project's rules and settings, read now. The error is the sentence to show: the
    /// rules file's problem, or the settings'.
    pub fn checking(&self) -> Result<Checking<'_>, String> {
        let rules = load_rules(&self.project).map_err(|problem| problem.to_string())?;
        let config = spellbook_config(&self.project).map_err(|e| e.to_string())?;
        Ok(Checking {
            wired: self,
            rules,
            exclude: config.exclude,
            runner: QueryRules::new(&self.grammars),
        })
    }
}

impl Checking<'_> {
    pub fn deps(&self) -> CheckDeps<'_> {
        CheckDeps {
            view: &self.wired.view,
            grammars: &self.wired.grammars,
            runner: &self.runner,
            report: &self.wired.report,
            clock: self.wired.clock.as_ref(),
            rules: &self.rules,
            exclude: &self.exclude,
        }
    }
}
