//! Everything checking needs for one project, put together: the rules read fresh, the settings,
//! and the runner over the project's grammars.

use crate::adapters::baseline_toml::load_baseline;
use crate::adapters::query_rules::QueryRules;
use crate::adapters::rules_toml::load_rules;
use crate::adapters::wiring::Wired;
use crate::domain::baseline::Baseline;
use crate::domain::config::spellbook_config;
use crate::domain::region::Region;
use crate::domain::rule::Rule;
use crate::domain::rule_settings::with_project_settings;
use crate::usecases::check_files::CheckDeps;
use crate::usecases::plan_fix::FixDeps;

pub struct Checking<'w> {
    wired: &'w Wired,
    /// The rules the project runs, with the severities `[spellbook.rules]` gives them.
    pub rules: Vec<Rule>,
    /// The rules `[spellbook.rules]` turned off.
    off: Vec<String>,
    exclude: Vec<String>,
    regions: Vec<Region>,
    /// `baseline.toml`: the findings that are known debt.
    pub baseline: Baseline,
    runner: QueryRules<'w>,
}

impl Wired {
    /// The project's rules (with `[spellbook.rules]` over them), regions, baseline and settings,
    /// read now. The error is the sentence to show: the rules file's problem, the regions file's,
    /// the baseline's, or the settings'.
    pub fn checking(&self) -> Result<Checking<'_>, String> {
        let rules = load_rules(&self.project).map_err(|problem| problem.to_string())?;
        let config = spellbook_config(&self.project).map_err(|e| e.to_string())?;
        let project_rules = with_project_settings(rules, &config.rules)?;
        let regions = self
            .regions
            .clone()
            .map_err(|problem| problem.to_string())?;
        let baseline = load_baseline(&self.project)?;
        Ok(Checking {
            wired: self,
            rules: project_rules.run,
            off: project_rules.off,
            regions,
            baseline,
            exclude: config.exclude,
            runner: QueryRules::new(&self.grammars),
        })
    }
}

impl Checking<'_> {
    /// What planning a fix reads: the plan's deps, these rules and their runner, the generators.
    pub fn fix_deps(&self) -> FixDeps<'_> {
        FixDeps {
            plan: self.wired.plan_deps(),
            runner: &self.runner,
            rules: &self.rules,
            generators: &self.wired.generators,
        }
    }

    /// The project's owned regions, read with the rules.
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    pub fn deps(&self) -> CheckDeps<'_> {
        CheckDeps {
            view: &self.wired.view,
            grammars: &self.wired.grammars,
            runner: &self.runner,
            report: &self.wired.report,
            clock: self.wired.clock.as_ref(),
            rules: &self.rules,
            off: &self.off,
            exclude: &self.exclude,
            regions: &self.regions,
            baseline: &self.baseline,
        }
    }
}
