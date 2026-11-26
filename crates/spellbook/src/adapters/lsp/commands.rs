//! The four commands the editor's pickers run, and the capabilities that announce them.
//!
//! - `spellbook.generators []`
//! - `spellbook.choices [generator, inputs, input]`
//! - `spellbook.plan [generator, inputs, fix?]`: with `fix` (`{ file, line, rule, index,
//!   version }`), the fix of that finding; a changed file comes with its `hunks`, `added` and
//!   `removed` (and `region` when a region's block is what changes), and the plan with the
//!   `fix` it is. The generator `sync` is the owned regions' refresh (`sync_on_save.rs`)
//! - `spellbook.apply [plan id]`: replies with `changed`, `written`, `skipped`, `refused`,
//!   `holes`, `first` (the first hole's file and line) and `stops`, the holes in the order to
//!   walk them; a plan's changed files are landed by the editor first (`landing.rs`)
//!
//! A reply is JSON the plugin reads; a refusal the person must hear is `{ "stop": "<sentence>" }`
//! in the reply. An error answers only a malformed or unknown command.

use std::collections::BTreeMap;

use std::path::Path;
use std::sync::Arc;

use lsp_server::{ErrorCode, ResponseError};
use lsp_types::{
    CodeActionProviderCapability, ExecuteCommandOptions, ExecuteCommandParams, MessageType,
    ServerCapabilities,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sorcerer_core::domain::project::Project;
use sorcerer_lsp::Reply;

use super::diagnostics::file_diagnostics_over;
use super::landing::{CANNOT_EDIT, changed_since_text};
use super::plans::KeptPlan;
use super::{NOT_TRUSTED, Server, lock};
use crate::adapters::wiring::{Wired, wired};
use crate::domain::config::{SPELLBOOK_IS_OFF, spellbook_config};
use crate::domain::inputs::{Given, InputStop, input_stop_text};
use crate::domain::plan::{Plan, PlanStop, plan_stop_text};
use crate::ports::generators::Generators;
use crate::ports::workspace::{WriteRefused, write_refused_text};
use crate::usecases::apply_plan::{AppliedGeneration, ChangesLanded, apply_plan};
use crate::usecases::choices_for::choices_for;
use crate::usecases::list_generators::list_generators;
use crate::usecases::plan_fix::{FindingAt, plan_fix};
use crate::usecases::plan_generation::plan_generation;
use crate::usecases::planned_view::PlannedView;
use crate::usecases::sync_regions::{SYNC_GENERATOR, SyncMode};

pub(super) const GENERATORS: &str = "spellbook.generators";
pub(super) const CHOICES: &str = "spellbook.choices";
pub(super) const PLAN: &str = "spellbook.plan";
pub(super) const APPLY: &str = "spellbook.apply";

/// What a sync asked to set a region up without naming it is told.
const INIT_NAMES_NO_REGION: &str =
    "a sync with mode init names the region to set up (region = NAME)";

/// What an apply of an unknown, expired or already applied plan is told.
const PLAN_GONE: &str = "that plan is gone (plans last ten minutes); make it again";

/// The four commands, and code actions (a finding's fixes).
pub(super) fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: [GENERATORS, CHOICES, PLAN, APPLY]
                .map(String::from)
                .to_vec(),
            ..ExecuteCommandOptions::default()
        }),
        ..ServerCapabilities::default()
    }
}

impl Server {
    /// Runs one of the commands and answers through `reply`: at once, or (an apply whose
    /// changes the editor lands) once the editor has answered the edit. A refusal the person
    /// must hear (an untrusted project, a plan that stopped) is a `stop` in the reply; an error
    /// answers only a malformed command.
    pub(super) fn execute(self: &Arc<Self>, params: Value, reply: Reply) {
        let params: ExecuteCommandParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(e) => return reply.send(Err(invalid(e))),
        };
        let command = params.command.as_str();
        if ![GENERATORS, CHOICES, PLAN, APPLY].contains(&command) {
            return reply.send(Err(invalid(format!("no command {command}"))));
        }
        let wired = match self.wired_for_commands() {
            Ok(wired) => wired,
            Err(sentence) => return reply.send(Ok(stop(&sentence))),
        };
        let arguments = &params.arguments;
        match command {
            GENERATORS => reply.send(Ok(self.generators(&wired))),
            CHOICES => reply.send(self.choices(&wired, arguments)),
            PLAN => reply.send(self.plan(&wired, arguments)),
            _ => self.apply(&wired, arguments, reply),
        }
    }

    /// The project's adapters, or the sentence that says why spellbook cannot run in it.
    pub(super) fn wired_for_commands(&self) -> Result<Wired, String> {
        let project: Project =
            (self.cfg.open)(&self.root).map_err(|e| format!("spellbook: {e:#}"))?;
        if !self.trusted(&project) {
            return Err(NOT_TRUSTED.to_string());
        }
        if !spellbook_config(&project)
            .map_err(|e| format!("spellbook: {e:#}"))?
            .enabled
        {
            return Err(SPELLBOOK_IS_OFF.to_string());
        }
        wired(project, self.cfg.cache.clone(), self.cfg.clock.clone())
            .map_err(|e| format!("spellbook: {e:#}"))
    }

    /// The generators for the picker; a broken one is left out and logged.
    fn generators(&self, wired: &Wired) -> Value {
        let mut summaries = Vec::new();
        for loaded in list_generators(&wired.generators) {
            match loaded {
                Ok(summary) => summaries.push(summary),
                Err(problem) => self
                    .client
                    .log(&format!("spellbook: a generator was left out: {problem}")),
            }
        }
        json!(summaries)
    }

    /// `[generator, inputs so far, input]`: the values the input may take now.
    fn choices(&self, wired: &Wired, arguments: &[Value]) -> Result<Value, ResponseError> {
        let name = text_argument(arguments, 0)?;
        let given = given_argument(arguments, 1);
        let input = text_argument(arguments, 2)?;
        let generator = match generator_named(wired, &name) {
            Ok(generator) => generator,
            Err(sentence) => return Ok(stop(&sentence)),
        };
        Ok(match choices_for(&wired.view, &generator, &given, &input) {
            Ok(choices) => json!({ "choices": choices }),
            Err(why) => stop(&input_stop_text(&why)),
        })
    }

    /// `[generator, inputs, fix?]`: the plan, kept for its apply; nothing is written. With a
    /// `fix`, the plan is the fix of the finding it names (planned by the fix's own generator)
    /// and keeps the document's version for the apply.
    fn plan(&self, wired: &Wired, arguments: &[Value]) -> Result<Value, ResponseError> {
        let name = text_argument(arguments, 0)?;
        let given = given_argument(arguments, 1);
        let fix = fix_argument(arguments, 2)?;
        let planned = match &fix {
            _ if name == SYNC_GENERATOR => self.planned_sync(wired, &given),
            Some(fix) => planned_fix(wired, fix, &given),
            None => planned_generation(wired, &name, &given),
        };
        Ok(match planned {
            Ok(plan) => {
                let reply = json!(plan);
                let version = fix.and_then(|fix| fix.version);
                lock(&self.plans).keep(KeptPlan { plan, version }, self.cfg.clock.now());
                reply
            }
            Err(stop_reply) => stop_reply,
        })
    }

    /// The sync the editor asks for by the generator name `sync`: the refresh of every region,
    /// or with `mode` `init` the region named by `region` set up as well. A region's quick fix
    /// sends its finding too; the sync needs only the inputs.
    ///
    /// Example: `["sync", { "region": "ports", "mode": "init", "file": "src/booking/ports/mod.rs" }]`
    /// plans `sync --init ports`.
    fn planned_sync(&self, wired: &Wired, given: &Given) -> Result<Plan, Value> {
        let mode = match (
            given.0.get("mode").map(String::as_str),
            given.0.get("region"),
        ) {
            (Some("init"), Some(region)) => SyncMode::Init(region.clone()),
            (Some("init"), None) => return Err(stop(INIT_NAMES_NO_REGION)),
            _ => SyncMode::Refresh,
        };
        self.sync_planned(wired, &mode)
            .map_err(|why| plan_stop_reply(&why))
    }

    /// `[plan id]`: lands the plan's changed files, writes its new files, marks each hole and
    /// opens the first one. A plan is applied once. The changed files are landed by the editor
    /// as one undoable edit when it can apply edits, and the rest waits for its answer;
    /// otherwise they are written to disk, and the person is told so once.
    fn apply(self: &Arc<Self>, wired: &Wired, arguments: &[Value], reply: Reply) {
        let id = match text_argument(arguments, 0) {
            Ok(id) => id,
            Err(e) => return reply.send(Err(e)),
        };
        let taken = lock(&self.plans).take(&id, self.cfg.clock.now());
        let Some(kept) = taken else {
            return reply.send(Ok(stop(PLAN_GONE)));
        };
        if kept.plan.changed_files().is_empty() {
            return reply.send(Ok(self.applied(wired, kept.plan, ChangesLanded::NotYet)));
        }
        if self.client.can_apply_edits() {
            return self.land_in_the_editor(kept, reply);
        }
        self.client
            .say_once("cannot-edit", MessageType::INFO, CANNOT_EDIT);
        reply.send(Ok(self.applied(wired, kept.plan, ChangesLanded::NotYet)));
    }

    /// Applies the plan (its changed files already landed by the editor, or not yet), publishes
    /// the marks of the changed and written files, opens the first hole, and says what was done.
    pub(super) fn applied(&self, wired: &Wired, plan: Plan, landed: ChangesLanded) -> Value {
        let applied = match apply_plan(&wired.apply_deps(), plan, landed) {
            Ok(applied) => applied,
            Err(e) => return stop(&format!("spellbook: {e:#}")),
        };
        if let Some((_, WriteRefused::Changed)) = &applied.refused
            && applied.changed.is_empty()
        {
            return stop(&changed_since_text(&applied.plan));
        }
        let stops = self.mark_files(wired, &applied);
        if let Some(first) = stops.first() {
            self.client.show_document(
                &self.root.join(first["file"].as_str().unwrap_or_default()),
                serde_json::from_value(first["range"].clone()).ok(),
            );
        }
        applied_reply(&applied, stops)
    }

    /// Publishes the marks and findings of each changed then written file, the changed ones
    /// read as the plan left them; returns the holes as tab stops, the files in that order and
    /// each file's holes by line then column.
    fn mark_files(&self, wired: &Wired, applied: &AppliedGeneration) -> Vec<Value> {
        let planned_view = PlannedView::new(&wired.view, changed_texts(&applied.plan));
        let mut stops = Vec::new();
        for file in applied.changed.iter().chain(&applied.written) {
            let mut diagnostics = file_diagnostics_over(wired, &planned_view, Path::new(file));
            stops.append(&mut diagnostics.stops);
            self.publish(file, diagnostics);
        }
        stops
    }
}

/// The text the plan leaves in each changed file, by path.
fn changed_texts(plan: &Plan) -> BTreeMap<String, String> {
    plan.changed_files()
        .into_iter()
        .map(|file| (file.path.clone(), file.text.clone()))
        .collect()
}

/// What apply did, as the plugin reads it.
fn applied_reply(applied: &AppliedGeneration, stops: Vec<Value>) -> Value {
    let refused = applied
        .refused
        .as_ref()
        .map(|(path, why)| json!({ "path": path, "why": write_refused_text(why) }));
    json!({
        "changed": applied.changed,
        "written": applied.written,
        "skipped": applied.skipped,
        "refused": refused,
        "holes": applied.plan.holes,
        "first": first_of(&stops),
        "stops": stops,
    })
}

/// The first stop's file and line (from 1), for an older plugin that reads `first`.
fn first_of(stops: &[Value]) -> Value {
    stops.first().map_or(Value::Null, |stop| {
        let line = stop["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
        json!({ "file": stop["file"], "line": line })
    })
}

/// A generator's plan, or the stop to reply with.
fn planned_generation(wired: &Wired, name: &str, given: &Given) -> Result<Plan, Value> {
    let generator = generator_named(wired, name).map_err(|sentence| stop(&sentence))?;
    plan_generation(&wired.plan_deps(), &generator, given).map_err(|why| plan_stop_reply(&why))
}

/// A fix's plan, or the stop to reply with (the rules that do not load, a finding gone, a fix
/// already applied).
fn planned_fix(wired: &Wired, fix: &FixArgument, given: &Given) -> Result<Plan, Value> {
    let checking = wired
        .checking()
        .map_err(|problem| stop(&format!("spellbook: {problem}")))?;
    let at = FindingAt {
        file: fix.file.clone(),
        line: fix.line,
        rule: fix.rule.clone(),
        index: fix.index,
    };
    plan_fix(&checking.fix_deps(), &at, given).map_err(|why| plan_stop_reply(&why))
}

fn generator_named(
    wired: &Wired,
    name: &str,
) -> Result<crate::domain::generator::Generator, String> {
    match wired.generators.named(name) {
        Some(Ok(generator)) => Ok(generator),
        Some(Err(problem)) => Err(problem.to_string()),
        None => Err(format!(
            "no generator named {name} (spellbook list shows them)"
        )),
    }
}

/// A stop reply: the sentence, and for a missing input which one and what it may be.
fn plan_stop_reply(why: &PlanStop) -> Value {
    match why {
        PlanStop::Input(input_stop) => input_stop_reply(input_stop),
        other => stop(&plan_stop_text(other)),
    }
}

fn input_stop_reply(why: &InputStop) -> Value {
    json!({
        "stop": input_stop_text(why),
        "input": why.input(),
        "choices": why.choices(),
    })
}

pub(super) fn stop(sentence: &str) -> Value {
    json!({ "stop": sentence })
}

fn text_argument(arguments: &[Value], at: usize) -> Result<String, ResponseError> {
    arguments
        .get(at)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| invalid(format!("argument {} should be text", at + 1)))
}

/// The finding a fix answers, as `plan`'s third argument names it: its file (relative to the
/// root, with `/`), its line (from 1), its rule, which of the rule's fixes (from 1), and the
/// document's version the editor knows.
///
/// Example: `{ "file": "src/bistro/booking/commands/close_table.txt", "line": 4, "rule":
/// "commands-use-ports", "index": 1, "version": 7 }`.
#[derive(Debug, Deserialize)]
struct FixArgument {
    file: String,
    line: u32,
    rule: Option<String>,
    index: usize,
    version: Option<i32>,
}

/// The fix argument; none when it is left out, null, or an empty Lua table.
fn fix_argument(arguments: &[Value], at: usize) -> Result<Option<FixArgument>, ResponseError> {
    match arguments.get(at) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) if items.is_empty() => Ok(None),
        Some(fix) => serde_json::from_value(fix.clone())
            .map(Some)
            .map_err(|e| invalid(format!("argument {} should be the fix: {e}", at + 1))),
    }
}

/// The inputs given so far: an object of text. An empty Lua table arrives as an empty list,
/// which is no inputs.
fn given_argument(arguments: &[Value], at: usize) -> Given {
    let pairs: BTreeMap<String, String> = arguments
        .get(at)
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .map(|(name, value)| {
                    let text = value
                        .as_str()
                        .map_or_else(|| value.to_string(), String::from);
                    (name.clone(), text)
                })
                .collect()
        })
        .unwrap_or_default();
    Given(pairs)
}

pub(super) fn invalid(why: impl ToString) -> ResponseError {
    ResponseError {
        code: ErrorCode::InvalidParams as i32,
        message: why.to_string(),
        data: None,
    }
}
