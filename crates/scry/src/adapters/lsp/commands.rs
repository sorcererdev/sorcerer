//! What the editor can do with a finding on a line: four code actions, each a command the
//! server runs.
//!
//! - `scry.why [id]` writes the finding in full to `.sorcerer/state/scry/why-<id>.md` and
//!   opens it;
//! - `scry.showCode [id]` opens the first piece of evidence with its lines selected;
//! - `scry.dismiss [id, "here" | "everywhere"]` dismisses it, republishes, and reviews its
//!   file again at once, so a finding waiting behind the limit of three can take the place.
//!
//! The actions on a line are found from what the server last published for that line, so
//! they always match the diagnostics the editor shows.

use std::sync::Arc;

use lsp_server::{ErrorCode, ResponseError};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, Command, ExecuteCommandOptions, ExecuteCommandParams, Position,
    Range, ServerCapabilities,
};
use serde_json::Value;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::domain::project::Project;
use sorcerer_lsp::path_of;

use super::{Server, lock};
use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::scry_toml::ScryToml;
use crate::adapters::why_text::why_text;
use crate::adapters::wiring::Wiring;
use crate::domain::finding::{Dismissal, Finding};
use crate::usecases::dismiss_finding::dismiss_finding;
use crate::usecases::explain_finding::explain_finding;
use crate::usecases::read_findings;
use crate::usecases::show_findings::Shown;

const WHY: &str = "scry.why";
const SHOW_CODE: &str = "scry.showCode";
const DISMISS: &str = "scry.dismiss";

/// Code actions and the three commands.
pub(super) fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec![WHY.into(), SHOW_CODE.into(), DISMISS.into()],
            ..ExecuteCommandOptions::default()
        }),
        ..ServerCapabilities::default()
    }
}

impl<W: Wiring + 'static> Server<W> {
    /// The actions for the findings on the request's line: for each, *Show why*, *Show code*
    /// (when it cites evidence), *Dismiss here* and *Dismiss everywhere*.
    ///
    /// The findings are the ones the server last published for the file, picked by the ids
    /// the editor hands back in the diagnostics it sends with the request (their `data`).
    /// An editor moves a diagnostic with the text between saves, so the id still finds the
    /// finding after lines were added above it; only when the editor sends no scry
    /// diagnostics are they picked by the line they were published on.
    pub(super) fn code_actions(&self, params: Value) -> Result<Value, ResponseError> {
        let params: CodeActionParams = serde_json::from_value(params).map_err(invalid)?;
        let Some(path) = path_of(&params.text_document.uri) else {
            return Ok(Value::Array(Vec::new()));
        };
        let published = lock(&self.published)
            .get(&path)
            .cloned()
            .unwrap_or_default();
        let actions: Vec<CodeActionOrCommand> = on_the_line(&published, &params)
            .into_iter()
            .flat_map(actions_for)
            .collect();
        serde_json::to_value(actions).map_err(invalid)
    }

    /// Runs one of the three commands.
    pub(super) fn execute(self: &Arc<Self>, params: Value) -> Result<Value, ResponseError> {
        let params: ExecuteCommandParams = serde_json::from_value(params).map_err(invalid)?;
        let id = argument(&params, 0)?;
        match params.command.as_str() {
            WHY => self.show_why(&id),
            SHOW_CODE => self.show_code(&id),
            DISMISS => {
                let reach = argument(&params, 1)?;
                self.dismiss(&id, dismissal_named(&reach)?)
            }
            other => Err(invalid(format!("no command {other}"))),
        }
        .map(|()| Value::Null)
    }

    /// Writes the finding in full beside the store and opens it.
    fn show_why(&self, id: &str) -> Result<(), ResponseError> {
        let project = self.project()?;
        let view = FilesView::new(Arc::new(project.clone()), self.cfg.cache.clone());
        let explanation = explain_finding(
            &project,
            &store_of(&project),
            &ScryToml::for_project(&project),
            &view,
            id,
        )
        .map_err(invalid)?;
        let state = project.state_dir().map_err(failed)?;
        let file = state.join(format!("scry/why-{}.md", explanation.finding.id.0));
        std::fs::create_dir_all(state.join("scry")).map_err(failed)?;
        std::fs::write(&file, why_text(&explanation)).map_err(failed)?;
        self.client.show_document(&file, None);
        Ok(())
    }

    /// Opens the file of the finding's first evidence with its lines selected.
    fn show_code(&self, id: &str) -> Result<(), ResponseError> {
        let project = self.project()?;
        let finding = finding_of(&project, id)?;
        let Some(first) = finding.evidence.first() else {
            return Err(invalid(format!("finding {id} cites no evidence")));
        };
        let selection = Range {
            start: Position {
                line: first.lines.start.saturating_sub(1),
                character: 0,
            },
            end: Position {
                line: first.lines.end,
                character: 0,
            },
        };
        self.client
            .show_document(&project.root.join(&first.file), Some(selection));
        Ok(())
    }

    /// Dismisses the finding at the clock's now, republishes, and reviews its file again.
    fn dismiss(self: &Arc<Self>, id: &str, dismissal: Dismissal) -> Result<(), ResponseError> {
        let project = self.project()?;
        // an unknown id is refused before anything is written
        let file = finding_of(&project, id)?.place.file;
        dismiss_finding(&store_of(&project), self.cfg.clock.now(), id, dismissal)
            .map_err(failed)?
            .map_err(invalid)?;
        self.publish_open();
        self.queue(project.root.join(file));
        Ok(())
    }

    /// The project, when the server may read it.
    fn project(&self) -> Result<Project, ResponseError> {
        let project = (self.cfg.open)(&self.root).map_err(failed)?;
        if !self.trusted(&project) {
            return Err(invalid(
                "this project is not trusted; run \"scry trust\" in it",
            ));
        }
        Ok(project)
    }
}

/// The published findings the request is about: those whose ids the editor's diagnostics
/// carry, else those published on the lines of its range.
fn on_the_line<'a>(published: &'a [Shown], params: &CodeActionParams) -> Vec<&'a Shown> {
    let ids: Vec<&str> = params
        .context
        .diagnostics
        .iter()
        .filter_map(|d| d.data.as_ref()?.as_str())
        .collect();
    let by_id: Vec<&Shown> = published
        .iter()
        .filter(|s| ids.contains(&s.finding.id.0.as_str()))
        .collect();
    if !by_id.is_empty() {
        return by_id;
    }
    let lines = params.range.start.line..=params.range.end.line;
    published
        .iter()
        .filter(|s| lines.contains(&s.placed.line.saturating_sub(1)))
        .collect()
}

/// The four actions on one finding, in order; *Show code* only when it cites evidence.
fn actions_for(s: &Shown) -> Vec<CodeActionOrCommand> {
    let id = Value::String(s.finding.id.0.clone());
    let here = Value::String("here".into());
    let everywhere = Value::String("everywhere".into());
    let show_code = (!s.finding.evidence.is_empty())
        .then(|| action("scry: Show code", SHOW_CODE, vec![id.clone()]));
    [
        Some(action("scry: Show why", WHY, vec![id.clone()])),
        show_code,
        Some(action(
            "scry: Dismiss here",
            DISMISS,
            vec![id.clone(), here],
        )),
        Some(action(
            "scry: Dismiss everywhere",
            DISMISS,
            vec![id, everywhere],
        )),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn action(title: &str, command: &str, arguments: Vec<Value>) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.into(),
        kind: Some(CodeActionKind::QUICKFIX),
        command: Some(Command {
            title: title.into(),
            command: command.into(),
            arguments: Some(arguments),
        }),
        ..CodeAction::default()
    })
}

/// The command's argument at this place, as text; a missing one is refused by name.
fn argument(params: &ExecuteCommandParams, at: usize) -> Result<String, ResponseError> {
    params
        .arguments
        .get(at)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| invalid(format!("{} needs argument {}", params.command, at + 1)))
}

fn dismissal_named(word: &str) -> Result<Dismissal, ResponseError> {
    match word {
        "here" => Ok(Dismissal::Here),
        "everywhere" => Ok(Dismissal::Everywhere),
        other => Err(invalid(format!(
            "{DISMISS}: \"{other}\" is not here or everywhere"
        ))),
    }
}

/// The stored finding with this id (or the start of one).
fn finding_of(project: &Project, id: &str) -> Result<Finding, ResponseError> {
    let stored = read_findings(&store_of(project)).map_err(failed)?;
    stored.find(id).cloned().map_err(invalid)
}

fn store_of(project: &Project) -> JsonFindingStore {
    JsonFindingStore::at(project.state.join("scry/findings.json"))
}

/// A request the server cannot carry out as asked: an unknown id, a missing argument.
fn invalid(why: impl ToString) -> ResponseError {
    ResponseError {
        code: ErrorCode::InvalidParams as i32,
        message: why.to_string(),
        data: None,
    }
}

/// A request that failed on the server's side: the project, the disk.
fn failed(why: impl std::fmt::Display) -> ResponseError {
    ResponseError {
        code: ErrorCode::InternalError as i32,
        message: format!("{why:#}"),
        data: None,
    }
}
