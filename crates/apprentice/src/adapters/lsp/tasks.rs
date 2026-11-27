//! Tasks from the editor: *do this* starts one (one at a time) on a thread of its own; its end
//! is a done message and a `✦` mark on the first changed line of each file it changed, with
//! *Show diff*, *Undo this task* and *Show journal*; saving every file it changed keeps it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_types::MessageType;
use serde_json::json;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;
use sorcerer_lsp::landing::{undo_edit, versioned_text_now};
use sorcerer_lsp::line_edits::first_changed_line;
use sorcerer_lsp::undo_plan::{FileDeletion, FileUndo, TakenBack, Undo, undo_of};
use sorcerer_lsp::uri_of;

use crate::adapters::jsonl_task_store::JsonlTaskStore;
use crate::adapters::markdown_journal::MarkdownJournal;
use crate::adapters::state_files::StateFiles;
use crate::adapters::task_runner::TaskRunner;
use crate::adapters::wiring::task_deps;
use crate::domain::config::apprentice_config;
use crate::domain::edits_text::FileEdit;
use crate::domain::log_text::title_of;
use crate::domain::task::{TaskRecord, TaskState};
use crate::ports::journal::Journal;
use crate::usecases::end_task::record_task_end;
use crate::usecases::offer_task::offer_text;
use crate::usecases::run_task::TaskOrder;

use super::editor_port::LspEditor;
use super::server_state::{Server, lock};

/// A finished task, still marked.
#[derive(Debug, Clone)]
pub(super) struct ShownTask {
    pub record: TaskRecord,
    pub files: Vec<ShownFile>,
}

/// One file a finished task changed.
#[derive(Debug, Clone)]
pub(super) struct ShownFile {
    pub relative: String,
    pub editor_path: PathBuf,
    pub new: bool,
    pub before: Option<String>,
    /// What the task last wrote.
    pub after: String,
    /// From 1.
    pub first_changed_line: u32,
    /// Saved, or undone: its mark is gone.
    pub settled: bool,
}

const BUSY: &str = "apprentice: a task is running; stop it first (apprentice: Stop this task)";

impl Server {
    /// The offer for an instruction: `apprentice: do this · ~$0.50 · sonnet`.
    pub(super) fn offer_title(&self) -> String {
        let Some(project) = self.project() else {
            return crate::domain::log_text::offer_title(None, "sonnet");
        };
        let config = apprentice_config(&project).unwrap_or_default();
        let (Ok(state), Ok(ledger)) = (
            StateFiles::for_project(&project),
            JsonlLedger::for_project(&project),
        ) else {
            return crate::domain::log_text::offer_title(None, &config.model);
        };
        offer_text(
            &JsonlTaskStore::at(state.tasks()),
            &ledger,
            self.cfg.clock.as_ref(),
            &config,
        )
    }

    /// *Do this* on a region: a task with the region's text as the instruction.
    pub(super) fn do_region(self: &Arc<Self>, id: &str) {
        let Some((_, record)) = self.record(id) else {
            return;
        };
        let path = self.root.join(&record.region.file);
        let text = self.text_now(&path);
        let line = record
            .region
            .found_in(&text)
            .map_or(record.region.lines.start, |span| span.start);
        let order = TaskOrder {
            instruction: record.region.text.trim().to_string(),
            file: record.region.file.clone(),
            text,
            line,
            unsaved: self.unsaved_edits(),
        };
        self.start_task(order);
    }

    /// Starts a task on a thread, unless one runs; its first look is every open file now, in
    /// place before the agent starts (its first `write_file` is checked against it). The
    /// check and the start are under one hold of `runner`, so two *do this* at once start one
    /// task, and the runner kept is always the running task's (a stop reaches it).
    pub(super) fn start_task(self: &Arc<Self>, order: TaskOrder) {
        let Some(project) = self.trusted_project() else {
            return;
        };
        let mut current = lock(&self.runner);
        if current
            .as_ref()
            .is_some_and(|runner| !runner.all_live().is_empty())
        {
            return self.client.say(MessageType::WARNING, BUSY);
        }
        let runner = match self.runner_for(&project) {
            Ok(runner) => runner,
            Err(why) => {
                return self
                    .client
                    .say(MessageType::ERROR, &format!("apprentice: {why:#}"));
            }
        };
        let first_look = self.open_texts();
        let started = runner.start_with(&order, |id| {
            lock(&self.task_looks).insert(id.to_string(), first_look);
        });
        let started = match started {
            Ok(started) => started,
            Err(refused) => return self.client.say(MessageType::WARNING, &refused.text()),
        };
        *current = Some(runner.clone());
        drop(current);
        std::thread::spawn(move || {
            runner.follow(started);
        });
    }

    fn runner_for(self: &Arc<Self>, project: &Project) -> anyhow::Result<Arc<TaskRunner>> {
        let editor = Arc::new(LspEditor {
            server: Arc::downgrade(self),
        });
        let agent = (self.cfg.agent)(project)?;
        let deps = task_deps(
            project,
            self.cfg.cache.clone(),
            editor,
            agent,
            self.cfg.clock.clone(),
        )?;
        Ok(Arc::new(TaskRunner::open(deps)?))
    }

    /// The open files whose text in the editor is not their disk text.
    fn unsaved_edits(&self) -> Vec<FileEdit> {
        self.open_texts()
            .into_iter()
            .filter_map(|(file, now)| {
                let disk = std::fs::read_to_string(self.root.join(&file)).unwrap_or_default();
                (disk != now).then(|| FileEdit {
                    path: sorcerer_core::domain::paths::forward_slashes(&file),
                    before: disk,
                    after: now,
                })
            })
            .collect()
    }

    /// *Stop this task*.
    pub(super) fn stop_task(&self, id: &str) {
        let live = lock(&self.runner)
            .as_ref()
            .and_then(|runner| runner.live(id));
        if let Some(live) = live {
            live.cancel.cancel();
        }
    }

    /// The running task's id, if one runs.
    pub(super) fn running_task(&self) -> Option<String> {
        let runner = lock(&self.runner).clone()?;
        runner.all_live().first().map(|live| live.id.clone())
    }

    /// The task ended: the progress line ends, the marks go on every file it changed, then the
    /// done message.
    pub(super) fn task_finished(&self, task: &TaskRecord, done_text: &str) {
        if let Some(line) = lock(&self.progress).remove(&task.id) {
            line.end(None);
        }
        self.client.clear_working();
        lock(&self.task_looks).remove(&task.id);
        lock(&self.told_unsaved).remove(&task.id);
        let written = lock(&self.written).remove(&task.id).unwrap_or_default();
        let journal = self.journal();
        let files: Vec<ShownFile> = task
            .files
            .iter()
            .filter_map(|file| {
                let after = written.get(Path::new(&file.path))?.clone();
                let before = journal
                    .as_ref()
                    .and_then(|journal| journal.before(&task.id, &file.path))
                    .flatten();
                Some(ShownFile {
                    relative: file.path.clone(),
                    editor_path: self.root.join(&file.path),
                    new: file.new,
                    first_changed_line: first_changed_line(before.as_deref().unwrap_or(""), &after)
                        .unwrap_or(1),
                    before,
                    after,
                    settled: false,
                })
            })
            .collect();
        let paths: Vec<PathBuf> = files.iter().map(|file| file.editor_path.clone()).collect();
        lock(&self.shown).push(ShownTask {
            record: task.clone(),
            files,
        });
        for path in paths {
            self.publish(&path);
        }
        // the marks first: the done message is the person's cue to look at them
        let kind = if task.state == TaskState::Done {
            MessageType::INFO
        } else {
            MessageType::WARNING
        };
        self.client.say(kind, done_text);
    }

    fn journal(&self) -> Option<MarkdownJournal> {
        let project = self.project()?;
        Some(MarkdownJournal::at(
            StateFiles::for_project(&project).ok()?.task_dirs(),
        ))
    }

    fn shown_task(&self, id: &str) -> Option<ShownTask> {
        lock(&self.shown)
            .iter()
            .find(|shown| shown.record.id == id)
            .cloned()
    }

    /// *Show diff*: the file's text before the task against now, in the editor's diff view
    /// (`sorcerer/showDiff`), else the before-text opened as a file.
    pub(super) fn show_diff(&self, id: &str, path: &Path) {
        let Some(shown) = self.shown_task(id) else {
            return;
        };
        let Some(file) = shown.files.iter().find(|file| file.editor_path == path) else {
            return;
        };
        let before = file.before.clone().unwrap_or_default();
        let title = format!(
            "apprentice: {} · before",
            title_of(&shown.record.instruction)
        );
        let params = json!({ "uri": uri_of(path), "before": before, "title": title });
        if self.client.notify_sorcerer("sorcerer/showDiff", params) {
            return;
        }
        if let Some(journal) = self.journal() {
            let before_file = journal
                .path(id)
                .with_file_name("before")
                .join(&file.relative);
            if before_file.exists() {
                return self.client.show_document(&before_file, None);
            }
        }
        self.client.say(
            MessageType::INFO,
            "apprentice: the file is new; there is no text before",
        );
    }

    /// *Show journal*.
    pub(super) fn show_journal(&self, id: &str) {
        if let Some(journal) = self.journal() {
            self.client.show_document(&journal.path(id), None);
        }
    }

    /// *Undo this task*: every file back to its text before, in one edit; a file the person
    /// edited since is left alone and named.
    pub(super) fn undo_task(self: &Arc<Self>, id: &str) {
        let Some(shown) = self.shown_task(id) else {
            return;
        };
        let deletion = if self.client.can_delete_files() {
            FileDeletion::Allowed
        } else {
            FileDeletion::NotAllowed
        };
        let mut undos: Vec<Undo> = Vec::new();
        let mut kept: Vec<String> = Vec::new();
        for file in shown.files.iter().filter(|file| !file.settled) {
            let taken = TakenBack {
                editor_path: &file.editor_path,
                new: file.new,
                before: file.before.as_deref(),
                after: &file.after,
            };
            match undo_of(
                &taken,
                versioned_text_now(&self.client, &file.editor_path),
                deletion,
            ) {
                FileUndo::Undo(undo) => undos.push(undo),
                FileUndo::Nothing => {}
                FileUndo::Edited => kept.push(file.relative.clone()),
            }
        }
        if undos.is_empty() {
            return self
                .client
                .say(MessageType::WARNING, &undo_refused_text(&kept));
        }
        let edit = undo_edit(undos.iter().map(Undo::undoing).collect());
        let server = Arc::clone(self);
        let id = id.to_string();
        self.client.apply_tool_edit(
            self.state_dir().as_deref(),
            &format!("apprentice: undo {}", title_of(&shown.record.instruction)),
            edit,
            move |answer| {
                if answer.applied {
                    server.task_undone(&id, &kept);
                } else {
                    server.client.say(
                        MessageType::WARNING,
                        "apprentice: the editor did not apply the undo",
                    );
                }
            },
        );
    }

    fn task_undone(&self, id: &str, kept: &[String]) {
        let Some(shown) = self.shown_task(id) else {
            return;
        };
        if let Some(project) = self.project()
            && let Ok(state) = StateFiles::for_project(&project)
        {
            record_task_end(
                &shown.record,
                TaskState::Undone,
                &JsonlTaskStore::at(state.tasks()),
                self.cfg.clock.as_ref(),
            );
        }
        lock(&self.shown).retain(|known| known.record.id != id);
        self.forget_said(id);
        for file in &shown.files {
            self.looked_at_own_edit(&file.editor_path, file.before.as_deref().unwrap_or(""));
            self.publish(&file.editor_path);
        }
        let mut said = format!("apprentice: undid {}", title_of(&shown.record.instruction));
        if !kept.is_empty() {
            said.push_str(&format!("; left {} as you edited them", kept.join(", ")));
        }
        self.client.say(MessageType::INFO, &said);
    }

    fn forget_said(&self, id: &str) {
        let paths: Vec<PathBuf> = {
            let mut said = lock(&self.said);
            said.values_mut()
                .for_each(|marks| marks.retain(|mark| mark.task != id));
            said.keys().cloned().collect()
        };
        for path in paths {
            self.publish(&path);
        }
    }

    /// A save of a file a finished task changed keeps it there; once every file is saved the
    /// task is recorded `kept` and its marks go.
    pub(super) fn keep_on_save(&self, path: &Path) {
        let mut kept: Vec<ShownTask> = Vec::new();
        {
            let mut shown = lock(&self.shown);
            for task in shown.iter_mut() {
                for file in task
                    .files
                    .iter_mut()
                    .filter(|file| file.editor_path == path)
                {
                    file.settled = true;
                }
            }
            shown.retain(|task| {
                let all = task.files.iter().all(|file| file.settled);
                if all {
                    kept.push(task.clone());
                }
                !all
            });
        }
        for task in kept {
            if let Some(project) = self.project()
                && let Ok(state) = StateFiles::for_project(&project)
            {
                record_task_end(
                    &task.record,
                    TaskState::Kept,
                    &JsonlTaskStore::at(state.tasks()),
                    self.cfg.clock.as_ref(),
                );
            }
            self.forget_said(&task.record.id);
        }
        self.publish(path);
    }

    /// The finished tasks marked on a line (from 1) of a file.
    pub(super) fn tasks_marked_at(&self, path: &Path, line: u32) -> Vec<String> {
        lock(&self.shown)
            .iter()
            .filter(|shown| {
                shown.files.iter().any(|file| {
                    !file.settled && file.editor_path == path && file.first_changed_line == line
                })
            })
            .map(|shown| shown.record.id.clone())
            .collect()
    }
}

fn undo_refused_text(kept: &[String]) -> String {
    if kept.is_empty() {
        "apprentice: nothing to undo; every file is as it was".to_string()
    } else {
        format!(
            "apprentice: not undone: you edited {} since",
            kept.join(", ")
        )
    }
}
