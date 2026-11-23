//! The four commands the editor's pickers run, and the capabilities that announce them.
//!
//! - `spellbook.generators []`
//! - `spellbook.choices [generator, inputs, input]`
//! - `spellbook.plan [generator, inputs]`
//! - `spellbook.apply [plan id]`
//!
//! A reply is JSON the plugin reads; a refusal the person must hear is `{ "stop": "<sentence>" }`
//! in the reply. An error answers only a malformed or unknown command.

use std::collections::BTreeMap;

use std::path::Path;

use lsp_server::{ErrorCode, ResponseError};
use lsp_types::{ExecuteCommandOptions, ExecuteCommandParams, Position, Range, ServerCapabilities};
use serde_json::{Value, json};
use sorcerer_core::domain::project::Project;

use super::diagnostics::file_diagnostics;
use super::{NOT_TRUSTED, Server, lock};
use crate::adapters::wiring::{Wired, wired};
use crate::domain::inputs::{Given, InputStop, input_stop_text};
use crate::domain::plan::{PlanStop, plan_stop_text};
use crate::ports::generators::Generators;
use crate::ports::workspace::write_refused_text;
use crate::usecases::apply_generation::{AppliedGeneration, apply_generation};
use crate::usecases::choices_for::choices_for;
use crate::usecases::list_generators::list_generators;
use crate::usecases::plan_generation::plan_generation;

pub(super) const GENERATORS: &str = "spellbook.generators";
pub(super) const CHOICES: &str = "spellbook.choices";
pub(super) const PLAN: &str = "spellbook.plan";
pub(super) const APPLY: &str = "spellbook.apply";

/// What an apply of an unknown, expired or already applied plan is told.
const PLAN_GONE: &str = "that plan is gone (plans last ten minutes); make it again";

/// The four commands, and no code actions.
pub(super) fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
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
    /// Runs one of the commands. A refusal the person must hear (an untrusted project, a plan
    /// that stopped) is a `stop` in the reply; an error answers only a malformed command.
    pub(super) fn execute(&self, params: Value) -> Result<Value, ResponseError> {
        let params: ExecuteCommandParams = serde_json::from_value(params).map_err(invalid)?;
        let command = params.command.as_str();
        if ![GENERATORS, CHOICES, PLAN, APPLY].contains(&command) {
            return Err(invalid(format!("no command {command}")));
        }
        let wired = match self.wired_for_commands() {
            Ok(wired) => wired,
            Err(sentence) => return Ok(stop(&sentence)),
        };
        let arguments = &params.arguments;
        match command {
            GENERATORS => Ok(self.generators(&wired)),
            CHOICES => self.choices(&wired, arguments),
            PLAN => self.plan(&wired, arguments),
            _ => self.apply(&wired, arguments),
        }
    }

    /// The project's adapters, or the sentence that says why spellbook cannot run in it.
    fn wired_for_commands(&self) -> Result<Wired, String> {
        let project: Project =
            (self.cfg.open)(&self.root).map_err(|e| format!("spellbook: {e:#}"))?;
        if !self.trusted(&project) {
            return Err(NOT_TRUSTED.to_string());
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

    /// `[generator, inputs]`: the plan, kept for its apply; nothing is written.
    fn plan(&self, wired: &Wired, arguments: &[Value]) -> Result<Value, ResponseError> {
        let name = text_argument(arguments, 0)?;
        let given = given_argument(arguments, 1);
        let generator = match generator_named(wired, &name) {
            Ok(generator) => generator,
            Err(sentence) => return Ok(stop(&sentence)),
        };
        match plan_generation(&wired.plan_deps(), &generator, &given) {
            Ok(plan) => {
                let reply = json!(plan);
                lock(&self.plans).keep(plan, self.cfg.clock.now());
                Ok(reply)
            }
            Err(why) => Ok(plan_stop_reply(&why)),
        }
    }

    /// `[plan id]`: writes the plan's new files, marks each hole and opens the first one. A
    /// plan is applied once.
    fn apply(&self, wired: &Wired, arguments: &[Value]) -> Result<Value, ResponseError> {
        let id = text_argument(arguments, 0)?;
        let taken = lock(&self.plans).take(&id, self.cfg.clock.now());
        let Some(plan) = taken else {
            return Ok(stop(PLAN_GONE));
        };
        let applied = match apply_generation(&wired.apply_deps(), plan) {
            Ok(applied) => applied,
            Err(e) => return Ok(stop(&format!("spellbook: {e:#}"))),
        };
        let first = self.mark_written_files(wired, &applied.written);
        if let Some((file, line)) = &first {
            self.client.show_document(
                &self.root.join(file),
                Some(Range::new(
                    Position::new(line - 1, 0),
                    Position::new(line - 1, 0),
                )),
            );
        }
        Ok(applied_reply(&applied, first))
    }

    /// Publishes the hole marks of each written file; returns the file and line of the first
    /// hole, by the order the files were written.
    fn mark_written_files(&self, wired: &Wired, written: &[String]) -> Option<(String, u32)> {
        let mut first = None;
        for file in written {
            let diagnostics = file_diagnostics(wired, Path::new(file));
            if first.is_none()
                && let Some(top) = diagnostics
                    .marks
                    .iter()
                    .map(|m| m.range.start.line + 1)
                    .min()
            {
                first = Some((file.clone(), top));
            }
            self.publish(file, diagnostics.all());
        }
        first
    }
}

/// What apply did, as the plugin reads it.
fn applied_reply(applied: &AppliedGeneration, first: Option<(String, u32)>) -> Value {
    let refused = applied
        .refused
        .as_ref()
        .map(|(path, why)| json!({ "path": path, "why": write_refused_text(why) }));
    json!({
        "written": applied.written,
        "skipped": applied.skipped,
        "refused": refused,
        "holes": applied.plan.holes,
        "first": first.map(|(file, line)| json!({ "file": file, "line": line })),
    })
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

fn stop(sentence: &str) -> Value {
    json!({ "stop": sentence })
}

fn text_argument(arguments: &[Value], at: usize) -> Result<String, ResponseError> {
    arguments
        .get(at)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| invalid(format!("argument {} should be text", at + 1)))
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
