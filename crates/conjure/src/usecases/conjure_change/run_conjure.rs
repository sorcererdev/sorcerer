//! `conjure_change`: the steps, and the day-budget check that comes before any call.

use sorcerer_core::domain::cost::spent_since;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::paths::forward_slashes;

use super::find_site::{Site, find_site};
use super::request_and_result::{
    ChangeDeps, ChangeRefused, ChangeRequest, ChangedFile, ConjuredChange,
};
use super::retry_drops::{TwoAnswers, drops_of_retry};
use super::rules_check::rules_broken_by;
use super::{check, situation, write};
use crate::domain::budget::day_allows;
use crate::domain::config::PRODUCT;
use crate::domain::edits::DropReason;
use crate::domain::record::{Attempt, change_id};
use crate::domain::reply::read_reply;
use crate::domain::retry_drops::CheckedDrop;
use crate::domain::spellbook_findings::{
    RuleFinding, RulesOutcome, TIMED_OUT_NOTE, broken_rules_text, held_back_text,
};

/// Conjures one change: where the cursor is, whether the day can pay, what the writer reads,
/// what it wrote, and whether that keeps the project's checked rules. An answer that breaks one
/// is not used: the writer is asked once more with the broken rules named, and when the second
/// answer breaks one too, the change is held back (no files, its goal says why). Nothing is
/// written to disk.
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
    let first = written_and_checked(&site, situation, Call::First, deps)?;
    let last = within_the_rules(&site, first, request.attempt, deps)?;
    Ok(ConjuredChange {
        id: change_id(deps.clock.now(), &request.file, request.line),
        unit_lines: site.unit.span,
        unit: site.unit.name,
        facts: site.facts,
        situation: last.situation,
        reply: last.reply,
        goal: last.goal,
        files: last.files,
        dropped: last.dropped,
        rules: last.rules,
        retry_drops: last.retry_drops,
        errand: last.errand,
    })
}

/// One answer: the situation the writer read, what it wrote, the files that would land, and
/// the rules those files break.
struct Answer {
    situation: String,
    reply: String,
    goal: Option<String>,
    files: Vec<ChangedFile>,
    dropped: Vec<(String, DropReason)>,
    broken: Vec<RuleFinding>,
    rules: RulesOutcome,
    /// What a retry dropped of the first answer; empty for a first answer.
    retry_drops: Vec<CheckedDrop>,
    errand: Errand,
}

/// Which call of a change this is, for the ledger.
#[derive(Clone, Copy)]
enum Call {
    First,
    /// The call after a rules check broke the first answer.
    Retry,
}

/// Asks the writer once on the situation and checks what it wrote.
fn written_and_checked(
    site: &Site,
    situation: String,
    call: Call,
    deps: &ChangeDeps,
) -> Result<Answer, ChangeRefused> {
    let asked = match call {
        Call::First => write::write_once(deps.writer, &situation, deps.cancel),
        Call::Retry => write::write_retry(deps.writer, &situation, deps.cancel),
    };
    let written = asked.map_err(|why| ChangeRefused::NotAnswered {
        why,
        unit: site.unit.name.clone(),
        facts: site.facts.clone(),
    })?;
    let reply = read_reply(&written.text);
    let (files, dropped) = check::checked_files(&reply, deps)?;
    let checked = rules_broken_by(&files, deps);
    let (goal, rules) = if checked.timed_out {
        (
            Some(match reply.goal {
                Some(goal) => format!("{goal} ({TIMED_OUT_NOTE})"),
                None => format!("({TIMED_OUT_NOTE})"),
            }),
            RulesOutcome::NotChecked(TIMED_OUT_NOTE.to_string()),
        )
    } else {
        (reply.goal, RulesOutcome::Kept)
    };
    Ok(Answer {
        situation,
        reply: written.text,
        goal,
        files,
        dropped,
        broken: checked.broken,
        rules,
        retry_drops: Vec::new(),
        errand: written.errand,
    })
}

/// The answer that keeps the rules: the first when it does; else a second, asked with the
/// broken rules named after the situation, carrying what it dropped of the first; else the
/// change held back. A day spent by the first answer holds it back without asking again.
fn within_the_rules(
    site: &Site,
    first: Answer,
    attempt: Attempt,
    deps: &ChangeDeps,
) -> Result<Answer, ChangeRefused> {
    if first.broken.is_empty() {
        return Ok(first);
    }
    if refuse_a_spent_day(attempt, deps).is_err() {
        return Ok(held_back(first));
    }
    let situation = format!(
        "{}{}",
        first.situation,
        broken_rules_text(&first.broken, |file, line| line_as_written(
            &first.files,
            file,
            line
        ))
    );
    let second = written_and_checked(site, situation, Call::Retry, deps)?;
    let errand = both_calls(&first.errand, &second.errand);
    let second = Answer { errand, ..second };
    if second.broken.is_empty() {
        let retry_drops = drops_of_retry(
            &TwoAnswers {
                first: &first.files,
                retry: &second.files,
                broken: &first.broken,
                retry_goal: second.goal.as_deref(),
            },
            deps,
        );
        let rules = match second.rules {
            RulesOutcome::NotChecked(why) => RulesOutcome::NotChecked(why),
            _ => RulesOutcome::AskedAgain(first.broken),
        };
        return Ok(Answer {
            rules,
            retry_drops,
            ..second
        });
    }
    Ok(held_back(second))
}

/// An answer whose files are not used: no files, and a goal naming the rules they broke.
fn held_back(answer: Answer) -> Answer {
    Answer {
        goal: Some(held_back_text(&answer.broken)),
        files: Vec::new(),
        rules: RulesOutcome::HeldBack(answer.broken.clone()),
        ..answer
    }
}

/// A line (from 1) of a file as the answer wrote it.
fn line_as_written(files: &[ChangedFile], file: &str, line: u32) -> Option<String> {
    let changed = files
        .iter()
        .find(|changed| forward_slashes(&changed.path) == file)?;
    changed
        .after
        .lines()
        .nth(line.checked_sub(1)? as usize)
        .map(str::to_string)
}

/// The second call's errand with both calls' cost and time.
fn both_calls(first: &Errand, second: &Errand) -> Errand {
    let cost_usd = match (first.cost_usd, second.cost_usd) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    };
    Errand {
        cost_usd,
        seconds: first.seconds + second.seconds,
        ..second.clone()
    }
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
