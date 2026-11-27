//! What the editor offers on a line, and the commands those offers run.
//!
//! On a line in a unit: *Conjure here* (`conjure.conjure [uri, line, endLine?]`). On a line
//! where scry has an open finding: *Conjure for this finding* (`conjure.conjure [uri, line,
//! endLine, findingId]`). On a mark's line: the mark's actions, `conjure: Undo`
//! (`conjure.undo [id]`), `conjure: Try harder` (`conjure.tryHarder [id]`), `conjure: Show
//! why` (`conjure.why [id]`) and `conjure: Show <file name>` (`conjure.show [id, path]`).
//! Lines in commands count from 1; lines in the protocol count from 0.
//!
//! An editor that cannot apply edits tied to a document's version is offered nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_server::{ErrorCode, ResponseError};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, Command, ExecuteCommandOptions, ExecuteCommandParams, Range,
    ServerCapabilities, Uri,
};
use serde_json::Value;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_lsp::path_of;

use super::changes::{LandedFile, names_told_apart};
use super::marks::{Mark, MarkKind};
use super::{Server, lock, project_file};
use crate::adapters::editor_view::EditorView;
use crate::adapters::scry_files::read_findings;
use crate::domain::config::conjure_config;
use crate::domain::scry_notes::{FindingsRead, StoredFinding};

pub(super) const CONJURE: &str = "conjure.conjure";
pub(super) const UNDO: &str = "conjure.undo";
pub(super) const TRY_HARDER: &str = "conjure.tryHarder";
pub(super) const WHY: &str = "conjure.why";
pub(super) const SHOW: &str = "conjure.show";

/// Code actions and the five commands.
pub(super) fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: [CONJURE, UNDO, TRY_HARDER, WHY, SHOW]
                .map(String::from)
                .to_vec(),
            ..ExecuteCommandOptions::default()
        }),
        ..ServerCapabilities::default()
    }
}

/// A change asked for: where, and for which scry finding.
#[derive(Debug, Clone)]
pub(crate) struct Ask {
    /// The file as the editor names it.
    pub path: PathBuf,
    /// The cursor's line, from 1.
    pub line: u32,
    /// The last selected line, from 1, when more than one line is selected.
    pub end_line: Option<u32>,
    pub finding: Option<String>,
}

impl Server {
    /// The actions on the request's lines, in order: the marked change's actions, *Conjure
    /// here*, then *Conjure for this finding* for each of scry's open findings there.
    pub(super) fn code_actions(&self, params: Value) -> Result<Value, ResponseError> {
        let params: CodeActionParams = serde_json::from_value(params).map_err(invalid)?;
        if !self.client.can_apply_edits() {
            return Ok(Value::Array(Vec::new()));
        }
        let Some(path) = path_of(&params.text_document.uri) else {
            return Ok(Value::Array(Vec::new()));
        };
        let uri = &params.text_document.uri;
        let (line, end_line) = lines_of(params.range);
        let mut actions = self.mark_actions(&path, &params);
        // an untrusted project's own grammar is never built or loaded: its files are not parsed,
        // and Conjure here is offered so that running it says how to trust the project
        if let Ok(project) = (self.cfg.open)(&self.root)
            && let Some(file) = project_file(&project, &path)
            && (!self.trusted(&project) || self.in_a_unit(&project, &path, &file, line))
        {
            actions.push(conjure_here(uri, line, end_line));
            actions.extend(
                findings_on(&project, &file, line, end_line, &params)
                    .iter()
                    .map(|finding| conjure_for(uri, line, end_line, &finding.id)),
            );
        }
        serde_json::to_value(actions).map_err(invalid)
    }

    /// Runs one of the five commands. What the person must hear (a refusal, a failure) is
    /// said in the editor; an error answers only a malformed command.
    pub(super) fn execute(self: &Arc<Self>, params: Value) -> Result<Value, ResponseError> {
        let params: ExecuteCommandParams = serde_json::from_value(params).map_err(invalid)?;
        match params.command.as_str() {
            CONJURE => self.conjure(ask_of(&params)?),
            UNDO => self.undo(&argument(&params, 0)?),
            TRY_HARDER => self.try_harder(&argument(&params, 0)?),
            WHY => self.show_why(&argument(&params, 0)?)?,
            SHOW => self.show_file(&argument(&params, 0)?, &argument(&params, 1)?)?,
            other => return Err(invalid(format!("no command {other}"))),
        }
        Ok(Value::Null)
    }

    /// Whether the line (from 1) is in one of the file's units, or in a region the parser lost, as the editor has the file now.
    fn in_a_unit(&self, project: &Project, path: &Path, file: &Path, line: u32) -> bool {
        let open: BTreeMap<PathBuf, String> = self
            .client
            .text_of(path)
            .map(|(_, text)| (file.to_path_buf(), text))
            .into_iter()
            .collect();
        let files = FilesView::new(Arc::new(project.clone()), self.cfg.cache.clone());
        let view = EditorView::new(files, open);
        view.unit_or_broken_region_at(file, line)
            .is_ok_and(|found| found.is_some())
    }

    /// The actions of the marks the request is about: those whose change id the editor's
    /// diagnostics carry, else those published on the request's lines.
    fn mark_actions(&self, path: &Path, params: &CodeActionParams) -> Vec<CodeActionOrCommand> {
        let published = lock(&self.published).get(path).cloned().unwrap_or_default();
        let Some(mark) = on_the_line(&published, params) else {
            return Vec::new();
        };
        let others = self.other_files(&mark);
        let offers = self.offers_for(&mark);
        actions_for(&mark, offers, &others)
    }

    /// What the mark's change offers now: Undo while it is open, Try harder too (or on a mark
    /// of no change) when the settings have it.
    fn offers_for(&self, mark: &Mark) -> Offers {
        let (undo, try_harder) = lock(&self.shown)
            .as_ref()
            .filter(|shown| shown.change.id == mark.change_id)
            .map_or((false, false), |shown| {
                (shown.offers_undo(), shown.offers_try_harder())
            });
        Offers {
            undo: undo && mark.kind != MarkKind::NoChange,
            try_harder: try_harder && self.offers_try_harder(),
        }
    }

    /// The changed files other than the one the mark is in, as `(path, file name)`.
    fn other_files(&self, mark: &Mark) -> Vec<(String, String)> {
        let shown = lock(&self.shown);
        let Some(shown) = shown.as_ref().filter(|s| s.change.id == mark.change_id) else {
            return Vec::new();
        };
        let files = &shown.change.files;
        let paths: Vec<String> = files.iter().map(LandedFile::relative_text).collect();
        files
            .iter()
            .zip(names_told_apart(&paths))
            .filter(|(file, _)| file.editor_path != mark.path)
            .map(|(file, name)| (file.relative_text(), name))
            .collect()
    }

    /// Whether the project's settings offer Try harder (they do unless `try_harder` is empty).
    fn offers_try_harder(&self) -> bool {
        (self.cfg.open)(&self.root)
            .ok()
            .and_then(|project| conjure_config(&project).ok())
            .is_none_or(|config| config.offers_try_harder())
    }
}

/// The cursor's line and, when more than one line is selected, the last one, both from 1. A
/// selection that ends at the start of a line does not hold that line.
fn lines_of(range: Range) -> (u32, Option<u32>) {
    let line = range.start.line + 1;
    let last = if range.end.character == 0 && range.end.line > range.start.line {
        range.end.line
    } else {
        range.end.line + 1
    };
    (line, (last > line).then_some(last))
}

/// scry's open (or waiting) findings in the file the request is about: those whose ids the
/// editor's scry diagnostics carry, and those placed on the request's lines.
fn findings_on(
    project: &Project,
    file: &Path,
    line: u32,
    end_line: Option<u32>,
    params: &CodeActionParams,
) -> Vec<StoredFinding> {
    let Ok(FindingsRead::Found(findings)) = read_findings(project) else {
        return Vec::new();
    };
    let ids: Vec<&str> = params
        .context
        .diagnostics
        .iter()
        .filter(|d| d.source.as_deref() == Some("scry"))
        .filter_map(|d| d.data.as_ref()?.as_str())
        .collect();
    let lines = line..=end_line.unwrap_or(line);
    findings
        .into_iter()
        .filter(|f| matches!(f.status.as_str(), "open" | "unverified" | "waiting"))
        .filter(|f| f.place.file == file)
        .filter(|f| ids.contains(&f.id.as_str()) || lines.contains(&f.place.lines.start))
        .collect()
}

/// The mark the request is about: one whose change id the editor's conjure diagnostics carry,
/// else one published on the request's lines.
fn on_the_line(published: &[Mark], params: &CodeActionParams) -> Option<Mark> {
    let ids: Vec<&str> = params
        .context
        .diagnostics
        .iter()
        .filter(|d| d.source.as_deref() == Some("conjure"))
        .filter_map(|d| d.data.as_ref()?.as_str())
        .collect();
    let by_id = published
        .iter()
        .find(|mark| ids.contains(&mark.change_id.as_str()));
    let lines = params.range.start.line..=params.range.end.line;
    by_id
        .or_else(|| {
            published
                .iter()
                .find(|mark| lines.contains(&mark.line.saturating_sub(1)))
        })
        .cloned()
}

/// Which of the actions that change the code a mark offers.
#[derive(Debug, Clone, Copy)]
struct Offers {
    undo: bool,
    try_harder: bool,
}

/// A mark's actions, in order: Undo, Try harder, Show why, then Show for each other file. A
/// mark of no change offers only Try harder and Show why; a change that can no longer be
/// undone, only the Show actions.
fn actions_for(
    mark: &Mark,
    offers: Offers,
    others: &[(String, String)],
) -> Vec<CodeActionOrCommand> {
    let id = Value::String(mark.change_id.clone());
    let undo = offers
        .undo
        .then(|| action("conjure: Undo", UNDO, vec![id.clone()]));
    let harder = offers
        .try_harder
        .then(|| action("conjure: Try harder", TRY_HARDER, vec![id.clone()]));
    let why = Some(action("conjure: Show why", WHY, vec![id.clone()]));
    let shows = others.iter().map(|(path, name)| {
        Some(action(
            &format!("conjure: Show {name}"),
            SHOW,
            vec![id.clone(), Value::String(path.clone())],
        ))
    });
    [undo, harder, why]
        .into_iter()
        .chain(shows)
        .flatten()
        .collect()
}

fn conjure_here(uri: &Uri, line: u32, end_line: Option<u32>) -> CodeActionOrCommand {
    let mut arguments = vec![Value::String(uri.as_str().to_string()), Value::from(line)];
    if let Some(end) = end_line {
        arguments.push(Value::from(end));
    }
    asking_action("Conjure here", arguments)
}

fn conjure_for(uri: &Uri, line: u32, end_line: Option<u32>, id: &str) -> CodeActionOrCommand {
    let arguments = vec![
        Value::String(uri.as_str().to_string()),
        Value::from(line),
        end_line.map_or(Value::Null, Value::from),
        Value::String(id.to_string()),
    ];
    asking_action("Conjure for this finding", arguments)
}

/// An action that asks for a change: a rewrite of the code where the cursor is.
fn asking_action(title: &str, arguments: Vec<Value>) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        ..command_action(title, CONJURE, arguments)
    })
}

/// An action on a mark: a fix of what the mark says.
fn action(title: &str, command: &str, arguments: Vec<Value>) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        kind: Some(CodeActionKind::QUICKFIX),
        ..command_action(title, command, arguments)
    })
}

fn command_action(title: &str, command: &str, arguments: Vec<Value>) -> CodeAction {
    CodeAction {
        title: title.into(),
        command: Some(Command {
            title: title.into(),
            command: command.into(),
            arguments: Some(arguments),
        }),
        ..CodeAction::default()
    }
}

/// `conjure.conjure`'s arguments: `[uri, line, endLine?, findingId?]`.
fn ask_of(params: &ExecuteCommandParams) -> Result<Ask, ResponseError> {
    let uri: Uri = argument(params, 0)?.parse().map_err(invalid)?;
    let path = path_of(&uri).ok_or_else(|| invalid(format!("{} is not a file", uri.as_str())))?;
    let number = |at: usize| {
        params
            .arguments
            .get(at)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    let line = number(1)
        .filter(|line| *line > 0)
        .ok_or_else(|| invalid(format!("{CONJURE} needs a line from 1 as argument 2")))?;
    Ok(Ask {
        path,
        line,
        end_line: number(2).filter(|end| *end > line),
        finding: params
            .arguments
            .get(3)
            .and_then(Value::as_str)
            .map(str::to_string),
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

/// A request the server cannot carry out as asked: an unknown id, a missing argument.
pub(super) fn invalid(why: impl ToString) -> ResponseError {
    ResponseError {
        code: ErrorCode::InvalidParams as i32,
        message: why.to_string(),
        data: None,
    }
}

/// A request that failed on the server's side: the project, the disk.
pub(super) fn failed(why: impl std::fmt::Display) -> ResponseError {
    ResponseError {
        code: ErrorCode::InternalError as i32,
        message: format!("{why:#}"),
        data: None,
    }
}
