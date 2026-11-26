//! The `Editor` port over the protocol: what a task's mediums become in the person's editor.
//! Every call comes from a task's thread (the follow loop, or a helper's loopback thread) and
//! may wait there; the main loop never waits.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use lsp_types::MessageType;
use serde_json::json;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_lsp::landing::{
    FileChange, NewFiles, OpenDocument, Snapshot, moved_on, workspace_edit,
};
use sorcerer_lsp::line_edits::{first_changed_line, in_base_line_endings, line_edits, same_text};
use sorcerer_lsp::uri_of;

use crate::adapters::disk_editor::slug_of;
use crate::adapters::state_files::StateFiles;
use crate::domain::edits_text::FileEdit;
use crate::domain::mediums::{Ask, MarkSeverity};
use crate::domain::task::TaskRecord;
use crate::ports::editor::{Asked, Editor, Written};

use super::server_state::{SaidMark, Server, lock};

/// How long the server waits for the editor to answer an edit, and then to report its text.
const APPLIED_WITHIN: Duration = Duration::from_secs(10);
const REPORTED_WITHIN: Duration = Duration::from_secs(3);

/// What the editor draws after the unit being written.
const WRITING: &str = "\u{27f3} apprentice: writing\u{2026}";

/// The editor, as a task's mediums see it.
pub(super) struct LspEditor {
    pub server: Weak<Server>,
}

impl LspEditor {
    fn server(&self) -> Option<Arc<Server>> {
        self.server.upgrade()
    }
}

impl Editor for LspEditor {
    fn notify(&self, text: &str) {
        if let Some(server) = self.server() {
            server.client.say(MessageType::INFO, text);
        }
    }

    fn mark(&self, task: &str, file: &Path, line: u32, text: &str, severity: MarkSeverity) {
        let Some(server) = self.server() else { return };
        let path = server.editor_path(file);
        let said = SaidMark {
            task: task.to_string(),
            line,
            text: text.to_string(),
            severity,
        };
        {
            let mut marks = lock(&server.said);
            let marks = marks.entry(path.clone()).or_default();
            marks.retain(|known| !(known.task == task && known.line == line));
            marks.push(said);
        }
        server.publish(&path);
    }

    fn ghost(&self, file: &Path, line: u32, lines: &[String]) {
        let Some(server) = self.server() else { return };
        let path = server.editor_path(file);
        let params =
            json!({ "uri": uri_of(&path), "line": line.saturating_sub(1), "lines": lines });
        if !server.client.notify_sorcerer("sorcerer/ghost", params) {
            let said = format!(
                "apprentice suggests at {}:{line}: {}",
                forward_slashes(file),
                lines.join(" / ")
            );
            server.client.say(MessageType::INFO, &said);
        }
    }

    fn write_file(&self, task: &str, file: &Path, text: &str) -> Written {
        match self.server() {
            Some(server) => server.write_for_task(task, file, text),
            None => Written::Refused("the editor's server has stopped".into()),
        }
    }

    fn note(&self, task: &str, title: &str, text: &str) -> sorcerer_core::Result<PathBuf> {
        let server = self.server().ok_or_else(stopped_error)?;
        let project = server.project().ok_or_else(stopped_error)?;
        let notes = StateFiles::for_project(&project)?.notes();
        std::fs::create_dir_all(&notes).map_err(|e| sorcerer_core::Error::io(&notes, e))?;
        let path = notes.join(format!("{task}-{}.md", slug_of(title)));
        std::fs::write(&path, format!("# {title}\n\n{text}\n"))
            .map_err(|e| sorcerer_core::Error::io(&path, e))?;
        server.open_note(&path);
        Ok(path)
    }

    fn ask(&self, task: &str, ask: &Ask, wait: Duration) -> Asked {
        match self.server() {
            Some(server) => server.ask_person(task, ask, wait),
            None => Asked::NoAnswer,
        }
    }

    fn edits_since_look(&self, task: &str) -> Vec<FileEdit> {
        match self.server() {
            Some(server) => server.edits_since_task_looked(task),
            None => Vec::new(),
        }
    }

    fn progress(&self, task: &str, text: &str) {
        let Some(server) = self.server() else { return };
        let mut progress = lock(&server.progress);
        match progress.get(task) {
            Some(line) => line.report(text),
            None => {
                let line = server.client.work_done("apprentice", text);
                progress.insert(task.to_string(), line);
            }
        }
    }

    fn finished(&self, task: &TaskRecord, done_text: &str) {
        if let Some(server) = self.server() {
            server.task_finished(task, done_text);
        }
    }
}

fn stopped_error() -> sorcerer_core::Error {
    sorcerer_core::Error::Config {
        file: PathBuf::from("apprentice"),
        why: "the editor's server has stopped".into(),
    }
}

impl Server {
    /// A task's `write_file`, landed through the editor at the buffer's version:
    ///
    /// 1. the file's text now (the editor's, else the disk's); refused as moved on when it is
    ///    not what the task last looked at;
    /// 2. one `workspace/applyEdit` (an edit at the version, or a created file), checked with
    ///    the host's `moved_on` first; waits for the editor's answer, then for it to report the
    ///    text;
    /// 3. the task's look, and the server's own look, move to the new text (so it is never
    ///    triaged as the person's), and the unit being written is drawn.
    pub(super) fn write_for_task(&self, task: &str, file: &Path, text: &str) -> Written {
        if !self.client.can_apply_edits() {
            return Written::Refused("the editor cannot apply edits from a server".into());
        }
        let path = self.editor_path(file);
        let now = self.client.text_of(&path);
        let before = now
            .as_ref()
            .map(|(_, text)| text.clone())
            .or_else(|| std::fs::read_to_string(&path).ok());
        if let Some(moved) = self.moved_since_task_looked(task, file, before.as_deref()) {
            return Written::MovedOn { now: moved };
        }
        if before.is_none() && !self.client.can_create_files() {
            return Written::Refused("the editor cannot create files from a server".into());
        }
        let after = match &before {
            Some(base) => in_base_line_endings(base, text),
            None => text.to_string(),
        };
        let snapshot = Snapshot {
            root: self.root.clone(),
            open: now
                .iter()
                .map(|(version, open_text)| {
                    let document = OpenDocument {
                        editor_path: path.clone(),
                        version: *version,
                        text: open_text.clone(),
                    };
                    (file.to_path_buf(), document)
                })
                .collect(),
        };
        let edits = line_edits(before.as_deref().unwrap_or(""), &after);
        let change = FileChange {
            path: file,
            new: before.is_none(),
            before: before.as_deref(),
            after: &after,
            edits: &edits,
        };
        if moved_on(&self.client, &snapshot, &[change]) {
            return Written::MovedOn {
                now: self.text_now(&path),
            };
        }
        let (edit, _) = workspace_edit(&[change], &snapshot, NewFiles::Created);
        let (sender, applied) = crossbeam_channel::bounded(1);
        self.client.apply_edit(
            &format!("apprentice: {}", forward_slashes(file)),
            edit,
            move |answer| {
                let _ = sender.send(answer);
            },
        );
        match applied.recv_timeout(APPLIED_WITHIN) {
            Ok(answer) if answer.applied => {}
            Ok(answer) => {
                return Written::Refused(
                    answer
                        .failure_reason
                        .unwrap_or_else(|| "the editor did not apply it".into()),
                );
            }
            Err(_) => return Written::Refused("the editor did not answer the edit".into()),
        }
        self.wait_until_reported(&path, &after);
        self.task_looked_at(task, file, &after);
        lock(&self.written)
            .entry(task.to_string())
            .or_default()
            .insert(file.to_path_buf(), after.clone());
        self.looked_at_own_edit(&path, &after);
        let line = first_changed_line(before.as_deref().unwrap_or(""), &after).unwrap_or(1);
        self.client.show_working(WRITING, &[(path, line)]);
        Written::Landed { before }
    }

    /// Some editors (Neovim) answer an edit before they report it.
    fn wait_until_reported(&self, path: &Path, after: &str) {
        let deadline = Instant::now() + REPORTED_WITHIN;
        while Instant::now() < deadline {
            if self
                .client
                .text_of(path)
                .is_some_and(|(_, text)| same_text(&text, after))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The file's text now when it is not what the task last looked at.
    fn moved_since_task_looked(
        &self,
        task: &str,
        file: &Path,
        now: Option<&str>,
    ) -> Option<String> {
        let looks = lock(&self.task_looks);
        let looked = looks.get(task)?.get(file)?;
        let now = now.unwrap_or_default();
        (!same_text(looked, now)).then(|| now.to_string())
    }

    fn task_looked_at(&self, task: &str, file: &Path, text: &str) {
        lock(&self.task_looks)
            .entry(task.to_string())
            .or_default()
            .insert(file.to_path_buf(), text.to_string());
    }

    /// The person's edits in open files since the task last looked; the look moves to now. A
    /// file the task has no look at yet (opened since it started) starts its look now.
    pub(super) fn edits_since_task_looked(&self, task: &str) -> Vec<FileEdit> {
        let open: Vec<(PathBuf, String)> = self
            .client
            .open_paths()
            .into_iter()
            .filter_map(|path| {
                let relative = path.strip_prefix(&self.root).ok()?.to_path_buf();
                let (_, text) = self.client.text_of(&path)?;
                Some((relative, text))
            })
            .collect();
        let mut looks = lock(&self.task_looks);
        let looked = looks.entry(task.to_string()).or_default();
        open.into_iter()
            .filter_map(
                |(file, now)| match looked.insert(file.clone(), now.clone()) {
                    Some(before) if !same_text(&before, &now) => Some(FileEdit {
                        path: forward_slashes(&file),
                        before,
                        after: now,
                    }),
                    _ => None,
                },
            )
            .collect()
    }

    /// Every open file of the project and its text: a task's first look.
    pub(super) fn open_texts(&self) -> HashMap<PathBuf, String> {
        self.client
            .open_paths()
            .into_iter()
            .filter_map(|path| {
                let relative = path.strip_prefix(&self.root).ok()?.to_path_buf();
                let (_, text) = self.client.text_of(&path)?;
                Some((relative, text))
            })
            .collect()
    }
}
