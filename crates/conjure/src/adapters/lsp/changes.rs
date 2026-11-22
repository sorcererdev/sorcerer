//! Conjuring a change in the editor, landing it, and its life after.
//!
//! `conjure.conjure` starts a change on a thread of its own (one at a time), under a progress
//! line. The thread opens the project, reads the open documents' versions and text (the
//! snapshot), runs `conjure_change` over the editor's view, and sends the change as one
//! `workspace/applyEdit`. When the editor answers that it applied the edit, the change is the
//! one shown: the change shown before it is recorded `kept`, and the new one's marks are
//! published. When the editor refuses, nothing is shown and the change is recorded `dropped`.
//!
//! A change shown is recorded once more, at the choice that ends it: `kept` (a changed file
//! saved, or the next change landed), `undone` (the Undo action, or the editor's own undo
//! bringing every file back), `replaced` (Try harder) or `unanswered` (the server shut down).
//! A change of no files is recorded `no-change` when it is shown, and its mark goes at the
//! next edit or save.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use lsp_server::ResponseError;
use lsp_types::MessageType;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::adapters::platform::paths::{canonical, names_ignore_case};
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Span;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_lsp::WorkDone;

use super::commands::Ask;
use super::landing::{NewFiles, OpenDocument, Snapshot, workspace_edit};
use super::marks::{mark_diagnostic, marks_of};
use super::{NOT_TRUSTED, Server, lock, project_relative};
use crate::adapters::editor_view::EditorView;
use crate::adapters::jsonl_change_store::JsonlChangeStore;
use crate::adapters::project_context::FilesContext;
use crate::domain::config::conjure_config;
use crate::domain::edits::NameCase;
use crate::domain::record::{
    Attempt, ChangeRecord, Choice, RECORD_VERSION, RecordedFacts, RecordedFile, Trigger, change_id,
};
use crate::ports::change_store::ChangeStore;
use crate::usecases::conjure_change::{
    ChangeDeps, ChangeRefused, ChangeRequest, ChangedFile, ConjuredChange, conjure_change,
};

/// The change marked now.
pub(super) struct Shown {
    pub change: LandedChange,
    /// An undo of it was sent and waits for the editor's answer, with the choice it records:
    /// the edits it makes are not the person's.
    pub undoing: Option<Choice>,
}

/// A change that landed in the editor (or a change of no files, shown as such), with what its
/// actions and its record need.
pub(super) struct LandedChange {
    pub id: String,
    pub goal: Option<String>,
    /// The model alias that wrote it.
    pub model: String,
    pub cost: Option<f64>,
    pub seconds: f64,
    /// The unit's first line, from 1.
    pub unit_line: u32,
    /// The file it was asked in, relative to the root.
    pub asked: PathBuf,
    /// The same file as the editor names it.
    pub asked_path: PathBuf,
    pub files: Vec<LandedFile>,
    /// The paths the writer named that were dropped (out of the project, fenced, excluded).
    pub not_written: Vec<String>,
    /// What the writer read and answered, for Show why.
    pub situation: String,
    pub reply: String,
    /// The ask, for Try harder.
    pub ask: Ask,
    pub project: Project,
    /// The record, written at the choice that ends the change.
    pub record: ChangeRecord,
}

/// One file of a landed change.
pub(super) struct LandedFile {
    /// Relative to the project's root.
    pub path: PathBuf,
    /// As the editor names it.
    pub editor_path: PathBuf,
    pub new: bool,
    /// The text before the change; none for a new file.
    pub before: Option<String>,
    pub after: String,
    /// The first and last changed lines in the new text, from 1.
    pub first_changed_line: u32,
    pub last_changed_line: u32,
    pub lines_changed: u32,
    pub parsed: bool,
    pub parse_errors: Vec<u32>,
}

impl LandedFile {
    /// The path relative to the root, written with `/`.
    pub fn relative_text(&self) -> String {
        forward_slashes(&self.path)
    }

    /// The file's name, as a mark says it: `cart__test.py`.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.relative_text())
    }
}

impl LandedChange {
    /// The file the main mark goes on: the one asked in when the change touched it, else the
    /// first; none for a change of no files.
    pub fn main_file(&self) -> Option<&LandedFile> {
        self.files
            .iter()
            .find(|file| file.path == self.asked)
            .or_else(|| self.files.first())
    }

    /// The changed files other than `main`, in the change's order.
    pub fn other_files<'a>(
        &'a self,
        main: &'a LandedFile,
    ) -> impl Iterator<Item = &'a LandedFile> + 'a {
        self.files.iter().filter(move |file| file.path != main.path)
    }

    pub fn goal_text(&self) -> String {
        self.goal
            .clone()
            .unwrap_or_else(|| "(no goal given)".into())
    }

    pub fn is_no_change(&self) -> bool {
        self.files.is_empty()
    }

    /// Whether the editor's file is one of the change's.
    fn touches(&self, path: &Path) -> bool {
        let resolved = canonical(path).ok();
        self.files.iter().any(|file| {
            file.editor_path == path
                || resolved.as_deref() == Some(self.project.root.join(&file.path).as_path())
        })
    }
}

/// A change conjured and not yet landed.
struct Conjured {
    change: ConjuredChange,
    snapshot: Snapshot,
    model: String,
    ask: Ask,
    project: Project,
    record: ChangeRecord,
}

/// Why no change is landed: the sentence the person reads, how it is shown, and the record of
/// a call that failed.
struct NotConjured {
    text: String,
    kind: MessageType,
    failed: Option<(Project, ChangeRecord)>,
}

impl NotConjured {
    fn said(kind: MessageType, text: impl Into<String>) -> NotConjured {
        NotConjured {
            text: text.into(),
            kind,
            failed: None,
        }
    }

    fn failed(why: impl std::fmt::Display) -> NotConjured {
        NotConjured::said(MessageType::ERROR, format!("conjure: {why:#}"))
    }
}

/// What a Try harder takes over from the change it replaces.
#[derive(Clone)]
pub(super) struct Again {
    /// The situation the first writer read, given as it was.
    pub situation: String,
    /// The id of the change it replaces.
    pub of: String,
}

impl Server {
    /// Conjures a change where the person asked, unless one is being conjured already.
    pub(super) fn conjure(self: &Arc<Self>, ask: Ask) {
        self.start(ask, None);
    }

    /// Starts conjuring on a thread of its own; refused (and said) while another runs.
    pub(super) fn start(self: &Arc<Self>, ask: Ask, again: Option<Again>) {
        let Some(cancel) = self.begin_running() else {
            self.client.say(MessageType::WARNING, BUSY);
            return;
        };
        // the text is read now, on the main loop: the edit is made at these versions, so typing
        // that comes after makes the editor refuse it rather than overwrite it
        let open = self.open_documents();
        let server = Arc::clone(self);
        std::thread::spawn(move || {
            server.conjure_and_land(&ask, again.as_ref(), open, &cancel);
        });
    }

    /// Every open document: the editor's path, the version and the text.
    fn open_documents(&self) -> Vec<(PathBuf, i32, String)> {
        self.client
            .open_paths()
            .into_iter()
            .filter_map(|path| {
                let (version, text) = self.client.text_of(&path)?;
                Some((path, version, text))
            })
            .collect()
    }

    /// The change is conjured and landed, or what stopped it is said (and a failed call
    /// recorded). The progress line ends when the editor has answered the edit.
    fn conjure_and_land(
        self: &Arc<Self>,
        ask: &Ask,
        again: Option<&Again>,
        open: Vec<(PathBuf, i32, String)>,
        cancel: &Cancel,
    ) {
        let progress = self.client.work_done("conjure", "conjuring a change");
        match self.conjured(ask, again, open, cancel) {
            Ok(conjured) => self.land(conjured, progress),
            Err(not) => {
                self.client.say(not.kind, &not.text);
                if let Some((project, record)) = not.failed {
                    self.record(&project, &record, Choice::Failed);
                }
                progress.end(None);
                self.end_running();
            }
        }
    }

    /// The change for the ask, over the editor's text as it is now.
    fn conjured(
        &self,
        ask: &Ask,
        again: Option<&Again>,
        open: Vec<(PathBuf, i32, String)>,
        cancel: &Cancel,
    ) -> Result<Conjured, NotConjured> {
        let project = (self.cfg.open)(&self.root).map_err(NotConjured::failed)?;
        if !self.trusted(&project) {
            return Err(NotConjured::said(MessageType::WARNING, NOT_TRUSTED));
        }
        let config = conjure_config(&project).map_err(NotConjured::failed)?;
        let Some(file) = project_relative(&project, &ask.path) else {
            return Err(NotConjured::said(
                MessageType::INFO,
                ChangeRefused::OutsideAUnit.text(),
            ));
        };
        let snapshot = snapshot_of(&project, open);
        let files = FilesView::new(Arc::new(project.clone()), self.cfg.cache.clone());
        let view: Arc<dyn CodebaseView> = Arc::new(EditorView::new(files, snapshot.texts()));
        let attempt = if again.is_some() {
            Attempt::TryHarder
        } else {
            Attempt::First
        };
        let writer = (self.cfg.wire)(&project, &config, attempt, view.clone())
            .map_err(NotConjured::failed)?;
        let fences = Fences::for_project(&project).map_err(NotConjured::failed)?;
        let ledger = JsonlLedger::for_project(&project).map_err(NotConjured::failed)?;
        let context = FilesContext {
            project: Arc::new(project.clone()),
            view: view.clone(),
            cache: self.cfg.cache.clone(),
            fences: fences.clone(),
        };
        let request = ChangeRequest {
            file,
            line: ask.line,
            selection: ask.end_line.map(|end| Span {
                start: ask.line,
                end,
            }),
            finding: ask.finding.clone(),
            attempt,
            situation: again.map(|again| again.situation.clone()),
        };
        let deps = ChangeDeps {
            project: &project,
            view: &*view,
            context: &context,
            fences: &fences,
            config: &config,
            writer: &*writer,
            ledger: &ledger,
            clock: &*self.cfg.clock,
            cancel,
            name_case: if names_ignore_case(&project.root) {
                NameCase::Ignored
            } else {
                NameCase::Matters
            },
        };
        let at = self.cfg.clock.now();
        let try_harder_of = again.map(|again| again.of.clone());
        match conjure_change(&request, &deps) {
            Ok(change) => Ok(Conjured {
                record: record_of(&change, &request, at, writer.model(), try_harder_of),
                model: writer.model().to_string(),
                change,
                snapshot,
                ask: ask.clone(),
                project,
            }),
            Err(ChangeRefused::NotAnswered { why, unit, facts }) => {
                let refused = ChangeRefused::NotAnswered {
                    why,
                    unit: unit.clone(),
                    facts: facts.clone(),
                };
                let record =
                    failed_record(&request, at, writer.model(), try_harder_of, &unit, &facts);
                Err(NotConjured {
                    text: refused.text(),
                    kind: MessageType::ERROR,
                    failed: Some((project, record)),
                })
            }
            Err(refused @ ChangeRefused::OutsideAUnit) => {
                Err(NotConjured::said(MessageType::INFO, refused.text()))
            }
            Err(refused) => Err(NotConjured::said(MessageType::WARNING, refused.text())),
        }
    }

    /// Sends the change to the editor as one edit; a change of no files is shown as such.
    fn land(self: &Arc<Self>, conjured: Conjured, progress: WorkDone) {
        if conjured.change.files.is_empty() {
            let landed = landed_change(conjured, &[]);
            self.record(&landed.project, &landed.record, Choice::NoChange);
            self.show(landed);
            progress.end(None);
            self.end_running();
            return;
        }
        let new_files = if self.client.can_create_files() {
            NewFiles::Created
        } else {
            NewFiles::LeftOut
        };
        let (edit, not_created) = workspace_edit(&conjured.change, &conjured.snapshot, new_files);
        let landed = landed_change(conjured, &not_created);
        if !not_created.is_empty() {
            self.client
                .say(MessageType::WARNING, &not_created_text(&not_created));
        }
        if landed.files.is_empty() {
            self.record(&landed.project, &landed.record, Choice::Dropped);
            progress.end(None);
            self.end_running();
            return;
        }
        let label = format!("conjure: {}", landed.goal_text());
        let server = Arc::clone(self);
        self.client.apply_edit(&label, edit, move |answer| {
            if answer.applied {
                server.show(landed);
            } else {
                server.client.say(MessageType::WARNING, REFUSED);
                server.record(&landed.project, &landed.record, Choice::Dropped);
            }
            progress.end(None);
            server.end_running();
        });
    }

    /// Makes a change the one shown: the one shown before it is recorded `kept` (unless it was
    /// already recorded), and the marks are replaced.
    fn show(&self, landed: LandedChange) {
        let shown = Shown {
            change: landed,
            undoing: None,
        };
        let before = lock(&self.shown).replace(shown);
        if let Some(before) = before {
            self.end_with(&before, Choice::Kept);
        }
        self.publish_marks();
    }

    /// Records the choice that ends a shown change; a change of no files was recorded when it
    /// was shown, and one being undone records its own choice.
    fn end_with(&self, shown: &Shown, choice: Choice) {
        if !shown.change.is_no_change() && shown.undoing.is_none() {
            self.record(&shown.change.project, &shown.change.record, choice);
        }
    }

    /// Publishes the shown change's marks, and clears every file that had marks and has none
    /// now.
    fn publish_marks(&self) {
        let marks = match lock(&self.shown).as_ref() {
            Some(shown) => marks_of(&shown.change)
                .into_iter()
                .map(|mark| {
                    let text = self.text_now(&mark.path);
                    let diagnostic = mark_diagnostic(&mark, &shown.change, &text);
                    (mark, diagnostic)
                })
                .collect(),
            None => Vec::new(),
        };
        let mut by_file: std::collections::HashMap<PathBuf, Vec<_>> =
            std::collections::HashMap::new();
        for (mark, diagnostic) in marks {
            by_file
                .entry(mark.path.clone())
                .or_default()
                .push((mark, diagnostic));
        }
        let gone: Vec<PathBuf> = {
            let mut published = lock(&self.published);
            let gone = published
                .keys()
                .filter(|path| !by_file.contains_key(*path))
                .cloned()
                .collect();
            *published = by_file
                .iter()
                .map(|(path, marks)| (path.clone(), marks.iter().map(|(m, _)| m.clone()).collect()))
                .collect();
            gone
        };
        for path in gone {
            self.client.publish(&path, Vec::new());
        }
        for (path, marks) in by_file {
            let diagnostics = marks.into_iter().map(|(_, d)| d).collect();
            self.client.publish(&path, diagnostics);
        }
    }

    /// Clears the shown change and its marks.
    fn clear_shown(&self) {
        *lock(&self.shown) = None;
        self.publish_marks();
    }

    /// The file's text as the editor has it, else as the disk has it, else empty.
    fn text_now(&self, path: &Path) -> String {
        self.client
            .text_of(path)
            .map(|(_, text)| text)
            .or_else(|| std::fs::read_to_string(path).ok())
            .unwrap_or_default()
    }

    /// Republishes a file's marks when it opens again.
    pub(super) fn republish(&self, path: &Path) {
        if lock(&self.published).contains_key(path) {
            self.publish_marks();
        }
    }

    /// The person changed a file: a mark of no change goes.
    pub(super) fn changed(&self, _path: &Path) {
        let no_change = lock(&self.shown)
            .as_ref()
            .is_some_and(|shown| shown.change.is_no_change());
        if no_change {
            self.clear_shown();
        }
    }

    /// The person saved a file: a change that touched it is kept; a mark of no change goes.
    pub(super) fn saved(&self, path: &Path) {
        let ended = {
            let mut shown = lock(&self.shown);
            let ends = shown.as_ref().is_some_and(|shown| {
                shown.undoing.is_none()
                    && (shown.change.is_no_change() || shown.change.touches(path))
            });
            if ends { shown.take() } else { None }
        };
        if let Some(ended) = ended {
            self.end_with(&ended, Choice::Kept);
            self.publish_marks();
        }
    }

    /// The editor is going: a running call is cancelled, a change still shown is recorded
    /// `unanswered`.
    pub(super) fn shutdown(&self) {
        if let Some(cancel) = lock(&self.running).as_ref() {
            cancel.cancel();
        }
        if let Some(shown) = lock(&self.shown).take() {
            self.end_with(&shown, Choice::Unanswered);
        }
    }

    pub(super) fn undo(self: &Arc<Self>, _id: &str) {}

    pub(super) fn try_harder(self: &Arc<Self>, _id: &str) {}

    pub(super) fn show_why(&self, _id: &str) -> Result<(), ResponseError> {
        Ok(())
    }

    pub(super) fn show_file(&self, _id: &str, _path: &str) -> Result<(), ResponseError> {
        Ok(())
    }

    /// Appends the record at its choice; a record that cannot be written is logged.
    fn record(&self, project: &Project, record: &ChangeRecord, choice: Choice) {
        let written = JsonlChangeStore::for_project(project).and_then(|store| {
            store.append(&ChangeRecord {
                choice,
                ..record.clone()
            })
        });
        if let Err(e) = written {
            self.client
                .log(&format!("conjure: the change was not recorded: {e}"));
        }
    }

    /// Takes the one place for a running change; `None` while another holds it.
    fn begin_running(&self) -> Option<Cancel> {
        let mut running = lock(&self.running);
        if running.is_some() {
            return None;
        }
        let cancel = Cancel::default();
        *running = Some(cancel.clone());
        Some(cancel)
    }

    fn end_running(&self) {
        *lock(&self.running) = None;
    }
}

/// The project's documents among the open ones, by path relative to the root.
fn snapshot_of(project: &Project, open: Vec<(PathBuf, i32, String)>) -> Snapshot {
    let open = open
        .into_iter()
        .filter_map(|(path, version, text)| {
            let relative = project_relative(project, &path)?;
            let document = OpenDocument {
                editor_path: path,
                version,
                text,
            };
            Some((relative, document))
        })
        .collect();
    Snapshot {
        root: project.root.clone(),
        open,
    }
}

/// The change as landed: its files without the new files left out, each with the editor's path.
fn landed_change(conjured: Conjured, not_created: &[PathBuf]) -> LandedChange {
    let Conjured {
        change,
        snapshot,
        model,
        ask,
        project,
        mut record,
    } = conjured;
    let files: Vec<LandedFile> = change
        .files
        .iter()
        .filter(|file| !not_created.contains(&file.path))
        .map(|file| landed_file(file, &snapshot))
        .collect();
    let not_written: Vec<String> = change
        .dropped
        .iter()
        .map(|(path, _)| path.clone())
        .collect();
    record.files = files.iter().map(recorded_file).collect();
    // what the editor could not create was not written either
    record.dropped = not_written
        .iter()
        .cloned()
        .chain(not_created.iter().map(|path| forward_slashes(path)))
        .collect();
    LandedChange {
        id: change.id,
        goal: change.goal,
        model,
        cost: change.errand.cost_usd,
        seconds: change.errand.seconds,
        unit_line: change.unit_lines.start,
        asked: record.file.clone().into(),
        asked_path: ask.path.clone(),
        files,
        not_written,
        situation: change.situation,
        reply: change.reply,
        ask,
        project,
        record,
    }
}

fn landed_file(file: &ChangedFile, snapshot: &Snapshot) -> LandedFile {
    let (first, last) = changed_lines(file);
    LandedFile {
        path: file.path.clone(),
        editor_path: snapshot.editor_path(&file.path),
        new: file.new,
        before: file.before.clone(),
        after: file.after.clone(),
        first_changed_line: first,
        last_changed_line: last,
        lines_changed: file.lines_changed,
        parsed: file.parsed,
        parse_errors: file.parse_errors.clone(),
    }
}

/// The first and last lines (from 1) of the new text that the edits wrote; a change that only
/// removed lines is the line where they were.
fn changed_lines(file: &ChangedFile) -> (u32, u32) {
    let mut shift: i64 = 0;
    let mut lines: Vec<(u32, u32)> = Vec::new();
    for edit in &file.edits {
        let start = (i64::from(edit.start_line) + shift) as u32;
        let written = edit.text.split_inclusive('\n').count() as u32;
        lines.push((start + 1, start + written.max(1)));
        shift += i64::from(written) - i64::from(edit.end_line - edit.start_line);
    }
    let first = lines.first().map_or(file.first_changed_line, |l| l.0);
    let last = lines.last().map_or(first, |l| l.1);
    (first, last)
}

fn recorded_file(file: &LandedFile) -> RecordedFile {
    RecordedFile {
        path: file.relative_text(),
        new: file.new,
        lines_changed: file.lines_changed,
        parse_errors: file.parse_errors.clone(),
    }
}

/// The record of a conjured change, its choice still to be made.
fn record_of(
    change: &ConjuredChange,
    request: &ChangeRequest,
    at: DateTime<Utc>,
    model: &str,
    try_harder_of: Option<String>,
) -> ChangeRecord {
    ChangeRecord {
        version: RECORD_VERSION,
        id: change.id.clone(),
        at,
        file: forward_slashes(&request.file),
        line: request.line,
        unit: change.unit.clone(),
        trigger: trigger_of(request),
        attempt: request.attempt,
        try_harder_of,
        facts: RecordedFacts::from(&change.facts),
        model: model.to_string(),
        goal: change.goal.clone(),
        files: Vec::new(),
        dropped: Vec::new(),
        seconds: change.errand.seconds,
        cost_usd: change.errand.cost_usd,
        choice: Choice::Kept,
    }
}

/// The record of a call that did not answer.
fn failed_record(
    request: &ChangeRequest,
    at: DateTime<Utc>,
    model: &str,
    try_harder_of: Option<String>,
    unit: &str,
    facts: &sorcerer_core::domain::facts::Facts,
) -> ChangeRecord {
    ChangeRecord {
        version: RECORD_VERSION,
        id: change_id(at, &request.file, request.line),
        at,
        file: forward_slashes(&request.file),
        line: request.line,
        unit: unit.to_string(),
        trigger: trigger_of(request),
        attempt: request.attempt,
        try_harder_of,
        facts: RecordedFacts::from(facts),
        model: model.to_string(),
        goal: None,
        files: Vec::new(),
        dropped: Vec::new(),
        seconds: 0.0,
        cost_usd: None,
        choice: Choice::Failed,
    }
}

fn trigger_of(request: &ChangeRequest) -> Trigger {
    request
        .finding
        .clone()
        .map_or(Trigger::Cursor, Trigger::Finding)
}

// What the person is told.

const BUSY: &str = "conjure: a change is already being conjured; this one was not started";
const REFUSED: &str = "conjure: the file changed while the change was conjured; your text stands";

fn not_created_text(paths: &[PathBuf]) -> String {
    let paths: Vec<String> = paths.iter().map(|path| forward_slashes(path)).collect();
    format!(
        "conjure: this editor cannot create files; not created: {}",
        paths.join(", ")
    )
}
