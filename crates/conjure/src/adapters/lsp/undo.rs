//! Taking a change back: the Undo action, and Try harder, which undoes and conjures again.
//!
//! An undo holds the place of the running change (`Server::running`) from the moment it is
//! planned until the editor answers it, and Try harder keeps holding it through the stronger
//! change, so nothing else lands meanwhile and every change keeps its record. A file is put
//! back only when its text is still the change's after-text; any other text is the person's,
//! and then the whole undo is refused and said.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lsp_types::MessageType;
use sorcerer_core::Cancel;

use super::changes::{
    Again, BUSY, LandedChange, LandedFile, OpenDocuments, REPORTED_WITHIN, ShownState,
    edited_after_text, emptied_text,
};
use super::commands::Ask;
use super::landing::{Undoing, undo_edit};
use super::{Server, lock};
use crate::domain::edits::{LineEdit, in_base_line_endings, line_edits, same_text};
use crate::domain::record::Choice;

/// What runs after an undo the editor applied, holding the running place it was given: Try
/// harder's next change, or letting the place go.
type Then = Box<dyn FnOnce(&Arc<Server>, Cancel) + Send>;

impl Server {
    /// Undoes the shown change: every file back to its text before, in one edit, recorded
    /// `undone`. Refused, and said, when a file was edited after the change landed, or while a
    /// change is being conjured.
    pub(super) fn undo(self: &Arc<Self>, id: &str) {
        let Some(cancel) = self.begin_running() else {
            self.client.say(MessageType::WARNING, BUSY);
            return;
        };
        let then: Then = Box::new(|server, _| server.end_running());
        self.take_back(id, Choice::Undone, cancel, then);
    }

    /// Undoes the shown change, recorded `replaced`, and conjures it again on the stronger
    /// model with the situation the first writer read. A change of no files needs no undo.
    pub(super) fn try_harder(self: &Arc<Self>, id: &str) {
        let Some(cancel) = self.begin_running() else {
            self.client.say(MessageType::WARNING, BUSY);
            return;
        };
        let Some((ask, again, no_change)) = self.again_for(id) else {
            return self.end_running();
        };
        if no_change {
            self.clear_shown();
            let open = self.open_documents();
            return self.start_again(ask, again, Some(open), cancel);
        }
        let then: Then =
            Box::new(move |server, cancel| server.start_again(ask, again, None, cancel));
        self.take_back(id, Choice::Replaced, cancel, then);
    }

    /// What Try harder takes over from the shown change, when it is the one named and offers
    /// Try harder.
    fn again_for(&self, id: &str) -> Option<(Ask, Again, bool)> {
        let shown = lock(&self.shown);
        let shown = shown
            .as_ref()
            .filter(|s| s.change.id == id && s.offers_try_harder())?;
        let change = &shown.change;
        let again = Again {
            situation: change.situation.clone(),
            of: id.to_string(),
            undone: change
                .files
                .iter()
                .map(|file| (file.editor_path.clone(), file.before.clone()))
                .collect(),
        };
        Some((change.ask.clone(), again, change.is_no_change()))
    }

    /// Conjures again on a thread, holding the running place. Without `open`, the text is read
    /// once the editor has reported the undo; when it never does, Try harder is refused rather
    /// than conjured over text it knows is old.
    fn start_again(
        self: &Arc<Self>,
        ask: Ask,
        again: Again,
        open: Option<OpenDocuments>,
        cancel: Cancel,
    ) {
        let server = Arc::clone(self);
        std::thread::spawn(move || {
            let open = open.or_else(|| server.documents_after_undo(&again));
            match open {
                Some(open) => server.conjure_and_land(&ask, Some(&again), open, &cancel),
                None => {
                    server.client.say(MessageType::WARNING, UNDO_NOT_REPORTED);
                    server.end_running();
                }
            }
        });
    }

    /// The open documents once each file of the undone change reads as before; `None` when the
    /// editor has not reported it within 3 seconds.
    fn documents_after_undo(&self, again: &Again) -> Option<OpenDocuments> {
        let reported = || {
            again.undone.iter().all(|(path, before)| {
                let now = self.client.text_of(path).map(|(_, text)| text);
                match before {
                    Some(before) => now.is_none_or(|text| text == *before),
                    None => now.is_none_or(|text| text.is_empty()),
                }
            })
        };
        let deadline = Instant::now() + REPORTED_WITHIN;
        while !reported() {
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Some(self.open_documents())
    }

    /// Sends the undo of the shown change, if it is the one named, offers Undo and every file is
    /// still at its text after the change; when the editor applied it, records the choice,
    /// clears the marks and runs `then`. Otherwise the running place is let go.
    fn take_back(self: &Arc<Self>, id: &str, choice: Choice, cancel: Cancel, then: Then) {
        let planned = {
            let mut shown = lock(&self.shown);
            let Some(shown) = shown
                .as_mut()
                .filter(|s| s.change.id == id && s.offers_undo())
            else {
                drop(shown);
                return self.end_running();
            };
            let planned = self.undo_plan(&shown.change);
            if planned.is_ok() {
                shown.state = ShownState::Undoing(choice);
            }
            planned
        };
        let plan = match planned {
            Ok(plan) => plan,
            Err(edited) => {
                self.client
                    .say(MessageType::WARNING, &edited_after_text(&edited));
                return self.end_running();
            }
        };
        if plan.undoings.is_empty() {
            return self.taken_back(id, choice, &plan.emptied, cancel, then);
        }
        let edit = undo_edit(plan.undoings.iter().map(Undo::undoing).collect());
        let server = Arc::clone(self);
        let id = id.to_string();
        self.client
            .apply_edit("conjure: undo", edit, move |answer| {
                if answer.applied {
                    server.taken_back(&id, choice, &plan.emptied, cancel, then);
                } else {
                    server.undo_refused(&id, &plan.edited_name);
                }
            });
    }

    /// What undoing the change takes, file by file; the name of the first file that was edited
    /// after the change landed, when one was.
    fn undo_plan(&self, change: &LandedChange) -> Result<UndoPlan, String> {
        let deletion = if self.client.can_delete_files() {
            FileDeletion::Allowed
        } else {
            FileDeletion::NotAllowed
        };
        let mut plan = UndoPlan {
            undoings: Vec::new(),
            emptied: Vec::new(),
            edited_name: change.main_file().map(LandedFile::name).unwrap_or_default(),
        };
        for file in &change.files {
            let now = self.versioned_text_now(&file.editor_path);
            match undo_of(file, now, deletion) {
                FileUndo::Nothing => {}
                FileUndo::Edited => return Err(file.name()),
                FileUndo::Undo(undo) => {
                    if matches!(undo, Undo::Empty { .. }) {
                        plan.emptied.push(file.relative_text());
                    }
                    plan.undoings.push(undo);
                }
            }
        }
        Ok(plan)
    }

    /// The editor applied the undo: the choice recorded, the marks cleared, then `then`.
    fn taken_back(
        self: &Arc<Self>,
        id: &str,
        choice: Choice,
        emptied: &[String],
        cancel: Cancel,
        then: Then,
    ) {
        let taken = {
            let mut shown = lock(&self.shown);
            match shown.as_ref() {
                Some(s) if s.change.id == id => shown.take(),
                _ => None,
            }
        };
        if let Some(taken) = taken {
            self.end_with(&taken, choice);
            self.publish_marks();
        }
        if !emptied.is_empty() {
            self.client
                .say(MessageType::WARNING, &emptied_text(emptied));
        }
        then(self, cancel);
    }

    /// The editor refused the undo (a file moved on before it arrived): the change stays shown
    /// and can be undone again.
    fn undo_refused(&self, id: &str, name: &str) {
        if let Some(shown) = lock(&self.shown).as_mut().filter(|s| s.change.id == id) {
            shown.state = ShownState::Open;
        }
        self.client
            .say(MessageType::WARNING, &edited_after_text(name));
        self.end_running();
    }
}

/// The undo of a change, file by file, and what the person is told.
struct UndoPlan {
    undoings: Vec<Undo>,
    /// New files emptied because the editor cannot delete files.
    emptied: Vec<String>,
    /// The file named if the editor refuses the undo.
    edited_name: String,
}

/// One file's undo.
enum Undo {
    /// Edits back to the text before, at the version the file has now.
    Edit {
        path: PathBuf,
        version: Option<i32>,
        edits: Vec<LineEdit>,
    },
    /// A new file emptied (the editor cannot delete files).
    Empty {
        path: PathBuf,
        version: Option<i32>,
        edits: Vec<LineEdit>,
    },
    Delete {
        path: PathBuf,
    },
}

impl Undo {
    fn undoing(&self) -> Undoing<'_> {
        match self {
            Undo::Edit {
                path,
                version,
                edits,
            }
            | Undo::Empty {
                path,
                version,
                edits,
            } => Undoing::Edit {
                path,
                version: *version,
                edits: edits.clone(),
            },
            Undo::Delete { path } => Undoing::Delete { path },
        }
    }
}

/// Whether the editor may delete a file the change created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileDeletion {
    Allowed,
    NotAllowed,
}

/// What undoing one file takes, from its text now.
enum FileUndo {
    Undo(Undo),
    /// The file is already as it was (the edit never reached it, or the person put it back).
    Nothing,
    /// The person edited it after the change landed.
    Edited,
}

/// One file's undo, from its version and text now (the editor's, else the disk's):
///
/// - at the change's after-text: back to its text before (a new file deleted, or emptied when
///   the editor cannot delete);
/// - already at its text before: nothing;
/// - a new file closed unsaved, so empty on disk (the editor created it empty): deleted when
///   the editor can;
/// - anything else is the person's, and is never overwritten.
fn undo_of(
    file: &LandedFile,
    now: Option<(Option<i32>, String)>,
    deletion: FileDeletion,
) -> FileUndo {
    let Some((version, text)) = now else {
        // in neither the editor nor the disk: only a new file may be gone
        return if file.new {
            FileUndo::Nothing
        } else {
            FileUndo::Edited
        };
    };
    let path = file.editor_path.clone();
    let before = file.before.as_deref().unwrap_or("");
    if !same_text(&text, &file.after) {
        return match (file.new, version, deletion) {
            (true, None, FileDeletion::Allowed) if text.is_empty() => {
                FileUndo::Undo(Undo::Delete { path })
            }
            _ if text == before => FileUndo::Nothing,
            _ => FileUndo::Edited,
        };
    }
    match (&file.before, deletion) {
        (Some(before), _) => FileUndo::Undo(Undo::Edit {
            path,
            version,
            // in the file's own line endings: an undo does not rewrite them
            edits: line_edits(&text, &in_base_line_endings(&text, before)),
        }),
        (None, FileDeletion::Allowed) => FileUndo::Undo(Undo::Delete { path }),
        (None, FileDeletion::NotAllowed) => FileUndo::Undo(Undo::Empty {
            path,
            version,
            edits: line_edits(&text, ""),
        }),
    }
}

const UNDO_NOT_REPORTED: &str =
    "conjure: the editor did not report the undo, so Try harder was not started";
