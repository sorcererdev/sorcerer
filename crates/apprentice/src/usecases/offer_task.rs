//! What an offer says before a task runs: the estimate (the median of the project's finished
//! tasks after five, else the start) and the model, or that the day's budget is spent.

use sorcerer_core::ports::ledger::Ledger;

use crate::domain::budget::{day_allows, spent_today};
use crate::domain::config::ApprenticeConfig;
use crate::domain::estimate::estimate_usd;
use crate::domain::log_text::offer_title;
use crate::domain::task::TaskState;
use crate::ports::clock::Clock;
use crate::ports::task_store::TaskStore;

/// The offer's title: `apprentice: do this · ~$0.50 · sonnet`, or `… · budget spent`.
pub fn offer_text(
    tasks: &dyn TaskStore,
    ledger: &dyn Ledger,
    clock: &dyn Clock,
    config: &ApprenticeConfig,
) -> String {
    let errands = ledger.read().map(|rows| rows.errands).unwrap_or_default();
    if !day_allows(spent_today(&errands, clock.now(), clock.offset()), config) {
        return offer_title(None, &config.model);
    }
    let finished: Vec<f64> = tasks
        .read()
        .map(|rows| rows.tasks)
        .unwrap_or_default()
        .iter()
        .filter(|task| matches!(task.state, TaskState::Done | TaskState::Kept))
        .map(|task| task.cost_usd)
        .collect();
    offer_title(
        Some(estimate_usd(&finished, config.estimate_start)),
        &config.model,
    )
}
