//! What lackey is working on, drawn where the person looks: each question being answered,
//! each claim being checked and the running task say so at the end of their line
//! (`⟳ lackey: researching…`) from the start of the work to its end, through
//! `sorcerer/working`; an answer or a check also has a line in the editor's status
//! (`$/progress`). Several things can be worked on at once, so every place is sent each time.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sorcerer_lsp::{WorkDone, WorkingPlace};

use crate::domain::region_record::RegionRecord;
use crate::domain::triage::Kind;
use crate::ports::reply_watch::ReplyWatch;

use super::server_state::{Server, lock};

/// What a task's line says before its agent has said what it does.
pub(super) const TASK_WORKING: &str = "\u{27f3} lackey: working\u{2026}";
const RESEARCHING: &str = "\u{27f3} lackey: researching\u{2026}";
const CHECKING: &str = "\u{27f3} lackey: checking\u{2026}";

/// One thing being worked on: where it is drawn, and its status line if it has one of its own.
pub(super) struct WorkingOn {
    place: WorkingPlace,
    status: Option<WorkDone>,
}

impl Server {
    /// Starts showing the work `key` (a task's id, a region's id) at a line (from 1) of a file.
    /// With a status text, the editor's status line says it too. Answers `false`, showing
    /// nothing new, when `key` is already being worked on.
    pub(super) fn work_begins(
        &self,
        key: &str,
        path: &Path,
        line: u32,
        text: &str,
        status: Option<&str>,
    ) -> bool {
        {
            let mut working = lock(&self.working);
            if working.contains_key(key) {
                return false;
            }
            let status = status.map(|status| self.client.work_done("lackey", status));
            working.insert(
                key.to_string(),
                WorkingOn {
                    place: place(path, line, text),
                    status,
                },
            );
        }
        self.send_working();
        true
    }

    /// The work `key` moved or says something new: `⟳ lackey: writing…` at the line it
    /// wrote, or what its agent does now. Nothing when `key` is not being worked on.
    pub(super) fn work_moves(&self, key: &str, at: Option<(PathBuf, u32)>, text: &str) {
        {
            let mut working = lock(&self.working);
            let Some(on) = working.get_mut(key) else {
                return;
            };
            if let Some((path, line)) = at {
                on.place.path = path;
                on.place.line = line;
            }
            on.place.text = text.to_string();
        }
        self.send_working();
    }

    /// The work `key` ended: its line and its status line are cleared.
    pub(super) fn work_ends(&self, key: &str) {
        let Some(ended) = lock(&self.working).remove(key) else {
            return;
        };
        if let Some(status) = ended.status {
            status.end(None);
        }
        self.send_working();
    }

    fn send_working(&self) {
        let mut places: Vec<(String, WorkingPlace)> = lock(&self.working)
            .iter()
            .map(|(key, on)| (key.clone(), on.place.clone()))
            .collect();
        places.sort_by(|a, b| a.0.cmp(&b.0));
        let places: Vec<WorkingPlace> = places.into_iter().map(|(_, place)| place).collect();
        self.client.show_working_each(&places);
    }
}

fn place(path: &Path, line: u32, text: &str) -> WorkingPlace {
    WorkingPlace {
        path: path.to_path_buf(),
        line,
        text: text.to_string(),
    }
}

/// The server as the answer use case's watch: a question says `researching…` on its line, a
/// claim `checking…`, and the status line names it.
pub(super) struct LineWatch {
    pub server: Arc<Server>,
    pub path: PathBuf,
    pub text: String,
}

impl ReplyWatch for LineWatch {
    fn begins(&self, record: &RegionRecord) -> bool {
        let line = record
            .region
            .found_in(&self.text)
            .map_or(record.region.lines.start, |found| found.start);
        let (inline, doing) = match record.acting_kind() {
            Some(Kind::Claim) => (CHECKING, "checking"),
            _ => (RESEARCHING, "researching"),
        };
        let status = format!("{doing}: {}", short(&record.region.text));
        self.server
            .work_begins(&record.region.id, &self.path, line, inline, Some(&status))
    }

    fn ends(&self, record: &RegionRecord) {
        self.server.work_ends(&record.region.id);
    }
}

/// The region's text on one line, cut at 40 characters: `why is the tax not in here?`.
fn short(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(40) {
        Some((at, _)) => format!("{}\u{2026}", &line[..at]),
        None => line,
    }
}
