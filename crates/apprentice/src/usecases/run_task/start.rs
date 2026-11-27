//! Starting a task: the day's cap, the record, the first message, the agent.

use std::sync::Arc;

use crate::domain::budget::{day_allows, spent_today};
use crate::domain::edits_text::{FileEdit, unsaved_edits_text};
use crate::domain::log_text::usd;
use crate::domain::prompt::{EDITOR_RULES, TaskPlace, task_prompt};
use crate::domain::task::{TaskRecord, TaskState, task_id};
use crate::ports::agent::{Running, TaskRequest, WayBack};

use super::task_deps::{LiveTask, TaskDeps};

/// What the person asked for and where.
#[derive(Debug, Clone)]
pub struct TaskOrder {
    /// The instruction in their words.
    pub instruction: String,
    /// The file, relative to the root with `/`, its text now, and the line, from 1.
    pub file: String,
    pub text: String,
    pub line: u32,
    /// The open files whose editor text is not their disk text.
    pub unsaved: Vec<FileEdit>,
}

/// A task that started: its shared state, and the agent running it.
pub struct StartedTask {
    pub live: Arc<LiveTask>,
    pub running: Box<dyn Running>,
}

/// Why a task did not start, in the sentence the person is told.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskRefused {
    DaySpent {
        spent: f64,
        cap: f64,
    },
    /// The agent could not be started (no `claude`, a ledger that cannot be read).
    NotStarted(String),
    /// The instruction is in a file the fences keep from every model (`.env`).
    Fenced(String),
}

impl TaskRefused {
    pub fn text(&self) -> String {
        match self {
            TaskRefused::DaySpent { spent, cap } => format!(
                "apprentice: today's budget is spent ({} of {}); [apprentice] budget_per_day raises it",
                usd(*spent),
                usd(*cap)
            ),
            TaskRefused::NotStarted(why) => format!("apprentice: the task did not start: {why}"),
            TaskRefused::Fenced(file) => {
                format!("apprentice: {file} is fenced; no task starts in it")
            }
        }
    }
}

/// Starts a task.
///
/// 1. Refuses when the day's budget is spent, or the instruction is in a fenced file.
/// 2. Records it `running` and journals the instruction.
/// 3. Builds its first message: the instruction, where it was written, the unit around it,
///    scry's open findings near it, the project's rules, the editor's unsaved edits (a fenced
///    file's are left out).
/// 4. Starts the agent, whose helper processes come back through `way_back`; a failure to start
///    is recorded `failed`.
pub fn start_task(
    order: &TaskOrder,
    way_back: WayBack,
    deps: &TaskDeps,
) -> Result<StartedTask, TaskRefused> {
    refuse_a_spent_day(deps)?;
    if !deps.fences.allows(&deps.root.join(&order.file)) {
        return Err(TaskRefused::Fenced(order.file.clone()));
    }
    let now = deps.clock.now();
    let record = TaskRecord::started(
        &task_id(now, &order.instruction),
        &order.instruction,
        &order.file,
        order.line,
        now,
    );
    let live = Arc::new(LiveTask::new(record.clone()));
    let _ = deps.tasks.append(&record);
    let _ = deps
        .journal
        .write(&live.id, "instruction", &order.instruction);
    let request = TaskRequest {
        task: live.id.clone(),
        prompt: task_prompt(&place_of(order, deps)),
        system: EDITOR_RULES.to_string(),
        model: deps.config.model.clone(),
        budget_usd: deps.config.budget_per_task,
        max_turns: deps.config.max_turns,
        root: deps.root.clone(),
        way_back,
    };
    match deps.agent.start(&request) {
        Ok(running) => Ok(StartedTask { live, running }),
        Err(why) => {
            let failed = live.update(|record| {
                record.state = TaskState::Failed;
                record.ended = Some(now);
                record.note = Some(why.to_string());
            });
            let _ = deps.tasks.append(&failed);
            Err(TaskRefused::NotStarted(why.to_string()))
        }
    }
}

fn refuse_a_spent_day(deps: &TaskDeps) -> Result<(), TaskRefused> {
    let errands = deps
        .ledger
        .read()
        .map_err(|e| TaskRefused::NotStarted(e.to_string()))?
        .errands;
    let spent = spent_today(&errands, deps.clock.now(), deps.clock.offset());
    if day_allows(spent, &deps.config) {
        Ok(())
    } else {
        Err(TaskRefused::DaySpent {
            spent,
            cap: deps.config.budget_per_day,
        })
    }
}

fn place_of(order: &TaskOrder, deps: &TaskDeps) -> TaskPlace {
    let file = std::path::Path::new(&order.file);
    TaskPlace {
        instruction: order.instruction.clone(),
        file: order.file.clone(),
        line: order.line,
        unit: deps.context.unit_at(file, &order.text, order.line),
        notes: deps.context.notes_near(file, &order.text, order.line),
        rules: deps.context.rules(),
        unsaved: unsaved_edits_text(&sendable(&order.unsaved, deps)),
    }
}

/// The unsaved edits a model may be sent: none in a fenced file.
fn sendable(unsaved: &[FileEdit], deps: &TaskDeps) -> Vec<FileEdit> {
    unsaved
        .iter()
        .filter(|edit| deps.fences.allows(&deps.root.join(&edit.path)))
        .cloned()
        .collect()
}
