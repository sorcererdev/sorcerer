//! Conjuring one change: find the unit, read the situation, ask the writer once, check what it
//! wrote. Nothing is written to disk; the caller lands the change (the editor's server through
//! `workspace/applyEdit`; the command line only prints it).

mod check;
mod situation;
mod write;

use std::path::PathBuf;

use sorcerer_core::Cancel;
use sorcerer_core::domain::cost::spent_since;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::facts::Facts;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::Language;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::{Span, Unit, unit_at};
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::budget::day_allows;
use crate::domain::config::{ConjureConfig, PRODUCT};
use crate::domain::edits::{DropReason, LineEdit, NameCase};
use crate::domain::record::{Attempt, change_id};
use crate::domain::reply::read_reply;
use crate::ports::clock::Clock;
use crate::ports::project_context::ProjectContext;
use crate::ports::writer::Writer;

/// What to conjure.
#[derive(Debug, Clone)]
pub struct ChangeRequest {
    /// Relative to the project root.
    pub file: PathBuf,
    /// The cursor's line, from 1.
    pub line: u32,
    /// The selected lines, when the person selected some.
    pub selection: Option<Span>,
    /// The id of the scry finding to conjure for.
    pub finding: Option<String>,
    pub attempt: Attempt,
    /// The situation text to reuse (Try harder); nothing is rebuilt when it is given.
    pub situation: Option<String>,
}

/// What the use case reads and calls.
pub struct ChangeDeps<'a> {
    pub project: &'a Project,
    pub view: &'a dyn CodebaseView,
    pub context: &'a dyn ProjectContext,
    pub fences: &'a Fences,
    pub config: &'a ConjureConfig,
    pub writer: &'a dyn Writer,
    pub ledger: &'a dyn Ledger,
    pub clock: &'a dyn Clock,
    pub cancel: &'a Cancel,
    /// Whether the project's file system ignores case: it decides which written paths are one
    /// file and how `exclude` matches.
    pub name_case: NameCase,
}

/// One file of a change.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub new: bool,
    /// The text the change started from; none for a new file.
    pub before: Option<String>,
    pub after: String,
    pub edits: Vec<LineEdit>,
    /// From 1.
    pub first_changed_line: u32,
    pub lines_changed: u32,
    /// Whether a grammar covers the file, so `parse_errors` says something.
    pub parsed: bool,
    pub parse_errors: Vec<u32>,
}

/// A conjured change: nothing of it is on disk yet. No files is "no change".
#[derive(Debug, Clone)]
pub struct ConjuredChange {
    pub id: String,
    pub unit: String,
    /// The unit's lines, from 1: a change with no files is marked on the first.
    pub unit_lines: Span,
    pub facts: Facts,
    /// The situation the writer read.
    pub situation: String,
    /// The writer's reply as it came.
    pub reply: String,
    pub goal: Option<String>,
    pub files: Vec<ChangedFile>,
    pub dropped: Vec<(String, DropReason)>,
    pub errand: Errand,
}

/// Why no change was conjured.
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeRefused {
    OutsideAUnit,
    DaySpent {
        budget: f64,
    },
    /// The writer was asked and did not answer (a timeout, a refusal, a cancel); the unit it
    /// was asked about and its facts, so the failure can be recorded.
    NotAnswered {
        why: String,
        unit: String,
        facts: Facts,
    },
    Failed(String),
}

impl ChangeRefused {
    /// The sentence the person reads.
    pub fn text(&self) -> String {
        match self {
            ChangeRefused::OutsideAUnit => "put the cursor in a function to conjure".into(),
            ChangeRefused::DaySpent { budget } => day_spent_text(*budget),
            ChangeRefused::NotAnswered { why, .. } => {
                format!("conjure: the model did not answer: {why}")
            }
            ChangeRefused::Failed(why) => format!("conjure: {why}"),
        }
    }
}

fn day_spent_text(budget: f64) -> String {
    format!(
        "conjure: today's budget (${budget:.2}) is spent. Conjuring resumes tomorrow; \"conjure cost\" shows the days, and [conjure] budget_per_day sets the budget."
    )
}

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

/// The place in the code a change is asked at.
struct Site {
    file: PathBuf,
    language: Language,
    /// The file's text as the view has it (the editor's when open).
    text: String,
    unit: Unit,
    /// The unit's lines, or the person's selection.
    span: Span,
    facts: Facts,
}

/// The unit under the cursor (or at the start of the selection) and its facts. A file of no
/// language, or a line in no unit, is `OutsideAUnit`.
fn find_site(request: &ChangeRequest, deps: &ChangeDeps) -> Result<Site, ChangeRefused> {
    let failed = |e: sorcerer_core::Error| ChangeRefused::Failed(e.to_string());
    if !deps.fences.allows(&request.file) {
        return Err(ChangeRefused::Failed(format!(
            "{} is fenced: a model is never shown it",
            forward_slashes(&request.file)
        )));
    }
    let language = deps
        .project
        .languages
        .for_path(&request.file)
        .ok_or(ChangeRefused::OutsideAUnit)?;
    let text = deps.view.text(&request.file).map_err(failed)?;
    let units = deps.view.units(&request.file).map_err(failed)?;
    let anchor = request.selection.map_or(request.line, |lines| lines.start);
    let unit = unit_at(&units, anchor)
        .ok_or(ChangeRefused::OutsideAUnit)?
        .clone();
    let facts = deps.view.facts(&request.file, &unit).map_err(failed)?;
    Ok(Site {
        file: request.file.clone(),
        span: request.selection.unwrap_or(unit.span),
        language: language.clone(),
        text,
        unit,
        facts,
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
