//! Watching the person's edits: each open file's text at the last look, the idle clock that
//! triages what changed since, and the save that answers. A checkout or a pull (`.git/HEAD`
//! moved) starts every look over and says nothing.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::Cancel;
use sorcerer_lsp::line_edits::same_text;

use super::server_state::{Server, lock};

impl Server {
    /// A file opened: its first look is its text on the disk, so what the person typed before
    /// the server saw the file is still theirs to triage, at a look scheduled as if they had
    /// just typed it. A file not on the disk starts at its text now.
    ///
    /// Example: the editor starts the server while the person types `def tax(cart): pass`, and
    /// opens the file with that line already in it; the look starts at the saved text without
    /// it, so the stub is found and offered once the idle time passes. Without this the line
    /// would be the first look itself, never a change, and never offered.
    pub(super) fn look_from_open(self: &Arc<Self>, path: &Path) {
        let Some((_, text)) = self.client.text_of(path) else {
            return;
        };
        let saved = std::fs::read_to_string(path).ok();
        let unsaved = saved.as_ref().is_some_and(|saved| !same_text(saved, &text));
        let first_look = saved.filter(|_| unsaved).unwrap_or(text);
        lock(&self.looks)
            .entry(path.to_path_buf())
            .or_insert(first_look);
        if unsaved {
            self.typed(path);
        }
    }

    /// The person typed: triage once they stop for the idle time.
    pub(super) fn typed(self: &Arc<Self>, path: &Path) {
        if self.relative(path).is_none() {
            return;
        }
        let server = Arc::clone(self);
        let owned = path.to_path_buf();
        self.jobs.submit(&idle_key(path), self.cfg.idle, move || {
            server.look_at(&owned)
        });
    }

    /// The person saved: what is waiting to be triaged is triaged now, then the file's questions
    /// are answered and its claims checked.
    pub(super) fn saved_file(self: &Arc<Self>, path: &Path) {
        if self.relative(path).is_none() {
            return;
        }
        self.jobs.cancel(&idle_key(path));
        let server = Arc::clone(self);
        let owned = path.to_path_buf();
        self.jobs.submit(
            &format!("save:{}", path.display()),
            std::time::Duration::ZERO,
            move || {
                server.look_at(&owned);
                server.answer_on_save(&owned);
            },
        );
    }

    /// One look at a file: what changed since the last look is triaged, the look moves to now,
    /// and the file's marks are published again (a region whose text changed loses its mark).
    pub(super) fn look_at(&self, path: &Path) {
        let Some(text) = self.client.text_of(path).map(|(_, text)| text) else {
            return;
        };
        let before = lock(&self.looks).insert(path.to_path_buf(), text.clone());
        if self.head_moved() {
            self.publish(path);
            return;
        }
        if let Some(before) = before.filter(|before| *before != text) {
            self.triage(path, &before, &text, &Cancel::default());
        }
        self.publish(path);
    }

    /// The server landed this text itself (a task's `write_file`): it is not the person's.
    pub(super) fn looked_at_own_edit(&self, path: &Path, text: &str) {
        lock(&self.looks).insert(path.to_path_buf(), text.to_string());
    }

    /// Whether `.git/HEAD` changed since the last look; when it did, every look starts over at
    /// the text now.
    fn head_moved(&self) -> bool {
        let now = std::fs::read_to_string(self.root.join(".git").join("HEAD")).ok();
        let before = std::mem::replace(&mut *lock(&self.head), now.clone());
        let moved = before.is_some() && before != now;
        if moved {
            let mut looks = lock(&self.looks);
            for (path, text) in looks.iter_mut() {
                if let Some((_, now)) = self.client.text_of(path) {
                    *text = now;
                }
            }
        }
        moved
    }
}

fn idle_key(path: &Path) -> String {
    format!("idle:{}", path.display())
}
