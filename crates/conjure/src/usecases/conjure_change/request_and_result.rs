//! What conjuring a change is asked, what it reads and calls, and what it gives back.

use std::path::PathBuf;

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::facts::Facts;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Span;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::config::ConjureConfig;
use crate::domain::edits::{DropReason, LineEdit, NameCase};
use crate::domain::record::Attempt;
use crate::domain::spellbook_findings::RulesOutcome;
use crate::ports::clock::Clock;
use crate::ports::project_context::ProjectContext;
use crate::ports::rule_checker::RuleChecker;
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
    /// The project's checked rules (spellbook's), run over a change before it lands.
    pub checker: &'a dyn RuleChecker,
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
    /// What conjure did about the project's checked rules: kept the first answer, asked once
    /// more, or held the change back (then `files` is empty and `goal` says why).
    pub rules: RulesOutcome,
    /// The call, or both calls when conjure asked again: their costs and times added.
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
