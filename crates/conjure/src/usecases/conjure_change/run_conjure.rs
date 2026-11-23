//! `conjure_change`: the four steps, and the day-budget check that comes before any call.

use sorcerer_core::domain::cost::spent_since;

use super::find_site::find_site;
use super::request_and_result::{ChangeDeps, ChangeRefused, ChangeRequest, ConjuredChange};
use super::{check, situation, write};
use crate::domain::budget::day_allows;
use crate::domain::config::PRODUCT;
use crate::domain::record::{Attempt, change_id};
use crate::domain::reply::read_reply;

/// Conjures one change, in four steps: where the cursor is, whether the day can pay, what the
/// writer reads, and what it wrote. Nothing is written to disk.
///
/// Example: the cursor on the stubbed body of `Cart.total` gives a change of two files, the
/// cart with its body filled and a new test file beside it.
pub fn conjure_change(
    request: &ChangeRequest,
    deps: &ChangeDeps,
) -> Result<ConjuredChange, ChangeRefused> {
    let site = find_site(request, deps)?;
    refuse_a_spent_day(request.attempt, deps)?;
    let situation = match &request.situation {
        Some(kept) => kept.clone(),
        None => situation::situation_for(&site, request, deps)?,
    };
    let written = write::write_once(deps.writer, &situation, deps.cancel).map_err(|why| {
        ChangeRefused::NotAnswered {
            why,
            unit: site.unit.name.clone(),
            facts: site.facts.clone(),
        }
    })?;
    let reply = read_reply(&written.text);
    let (files, dropped) = check::checked_files(&reply, deps)?;
    Ok(ConjuredChange {
        id: change_id(deps.clock.now(), &request.file, request.line),
        unit_lines: site.unit.span,
        unit: site.unit.name,
        facts: site.facts,
        situation,
        reply: written.text,
        goal: reply.goal,
        files,
        dropped,
        errand: written.errand,
    })
}

/// Refuses before any call when today's spending leaves no room for this attempt.
fn refuse_a_spent_day(attempt: Attempt, deps: &ChangeDeps) -> Result<(), ChangeRefused> {
    let errands = deps
        .ledger
        .read()
        .map_err(|e| ChangeRefused::Failed(e.to_string()))?
        .errands;
    let offset = deps.clock.offset();
    let today = deps.clock.now().with_timezone(&offset).date_naive();
    let spent = spent_since(&errands, PRODUCT, today, offset);
    if day_allows(spent, deps.config.budget_per_day, attempt) {
        return Ok(());
    }
    Err(ChangeRefused::DaySpent {
        budget: deps.config.budget_per_day,
    })
}
