//! Regions in the editor: triage on idle, answers on save, and what the person does with a
//! region (show the code, keep the answer, dismiss, label, do it).

use std::path::Path;
use std::sync::Arc;

use lsp_types::{MessageType, Position, Range};
use serde_json::json;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_lsp::landing::{FileChange, NewFiles, OpenDocument, Snapshot, workspace_edit};
use sorcerer_lsp::line_edits::line_edits;

use crate::adapters::grammar_syntax::GrammarSyntax;
use crate::adapters::json_region_store::JsonRegionStore;
use crate::adapters::state_files::StateFiles;
use crate::domain::config::{ApprenticeConfig, apprentice_config};
use crate::domain::region_record::RegionRecord;
use crate::domain::triage::Kind;
use crate::ports::region_store::RegionStore;
use crate::ports::syntax::Syntax;
use crate::usecases::answer_regions::{AnswerDeps, answer_saved};
use crate::usecases::label_region::{dismiss_region, label_region};
use crate::usecases::triage_regions::{Look, TriageDeps, triage_change};

use super::server_state::Server;

impl Server {
    /// The project's region store.
    pub(super) fn region_store(&self, project: &Project) -> Option<Arc<dyn RegionStore>> {
        let state = StateFiles::for_project(project).ok()?;
        Some(Arc::new(JsonRegionStore::at(
            state.regions(),
            state.labels(),
        )))
    }

    pub(super) fn syntax(&self, project: &Project) -> GrammarSyntax {
        GrammarSyntax {
            languages: project.languages.clone(),
            cache: self.cfg.cache.clone(),
        }
    }

    fn config(project: &Project) -> ApprenticeConfig {
        apprentice_config(project).unwrap_or_default()
    }

    /// Triage of one change, through the use case; a fenced file is never triaged (its text
    /// would reach Jev).
    pub(super) fn triage(&self, path: &Path, before: &str, after: &str, cancel: &Cancel) {
        let (Some(project), Some(file)) = (self.trusted_project(), self.relative(path)) else {
            return;
        };
        if !self.may_send(path) {
            return;
        }
        let (Some(regions), Ok(ledger)) = (
            self.region_store(&project),
            JsonlLedger::for_project(&project),
        ) else {
            return;
        };
        let deps = TriageDeps {
            syntax: Arc::new(self.syntax(&project)),
            regions,
            decider: (self.cfg.decider)(&project),
            ledger: Arc::new(ledger),
            clock: self.cfg.clock.clone(),
            config: Self::config(&project),
        };
        let look = Look {
            file: &file,
            before,
            after,
        };
        triage_change(&look, &deps, cancel);
    }

    /// The saved file's questions answered and claims checked, then its marks published; a
    /// fenced file has none.
    pub(super) fn answer_on_save(&self, path: &Path) {
        let (Some(project), Some(file)) = (self.trusted_project(), self.relative(path)) else {
            return;
        };
        if !self.may_send(path) {
            return;
        }
        let (Some(regions), Ok(ledger), Ok(fences)) = (
            self.region_store(&project),
            JsonlLedger::for_project(&project),
            Fences::for_project(&project),
        ) else {
            return;
        };
        let view = self.cfg.disk_view(&project);
        let deps = AnswerDeps {
            researcher: (self.cfg.researcher)(&project, view.clone()),
            decider: (self.cfg.decider)(&project),
            fences,
            view,
            languages: project.languages.clone(),
            regions,
            ledger: Arc::new(ledger),
            clock: self.cfg.clock.clone(),
            config: Self::config(&project),
        };
        let text = self.text_now(path);
        let replied = answer_saved(&file, &text, &deps, &Cancel::default());
        for record in replied
            .iter()
            .filter(|record| record.reply.as_ref().is_some_and(|reply| reply.long))
        {
            let reply = record.reply.as_ref().expect("filtered");
            let _ = self.write_note(&record.region.id, &record.region.text, &reply.text);
        }
        self.publish(path);
    }

    /// The records of a file in the editor now, each with where its text is (from 1).
    pub(super) fn records_found(&self, path: &Path) -> Vec<(RegionRecord, u32, u32)> {
        let (Some(project), Some(file)) = (self.project(), self.relative(path)) else {
            return Vec::new();
        };
        let Some(regions) = self.region_store(&project) else {
            return Vec::new();
        };
        let text = self.text_now(path);
        regions
            .of_file(&file)
            .into_iter()
            .filter_map(|record| {
                let found = record.region.found_in(&text)?;
                Some((record, found.start, found.end))
            })
            .collect()
    }

    /// A record by its id.
    pub(super) fn record(&self, id: &str) -> Option<(Project, RegionRecord)> {
        let project = self.project()?;
        let record = self.region_store(&project)?.get(id)?;
        Some((project, record))
    }

    /// *Show code*: the first place the answer cites.
    pub(super) fn show_code(&self, id: &str) {
        let Some((_, record)) = self.record(id) else {
            return;
        };
        let Some((file, line)) = record
            .reply
            .and_then(|reply| reply.citations.into_iter().next())
        else {
            return;
        };
        let at = Position::new(line.saturating_sub(1), 0);
        self.client
            .show_document(&self.root.join(file), Some(Range { start: at, end: at }));
    }

    /// *Keep this answer*: the answer written in below the region, as a comment in the file's
    /// language (else as a paragraph), through the editor as one undoable edit.
    pub(super) fn keep_answer(&self, id: &str) {
        let Some((project, record)) = self.record(id) else {
            return;
        };
        let Some(reply) = &record.reply else { return };
        let path = self.root.join(&record.region.file);
        let Some((version, text)) = self.client.text_of(&path) else {
            return self.client.say(
                MessageType::WARNING,
                "apprentice: open the file to keep the answer in it",
            );
        };
        let Some(found) = record.region.found_in(&text) else {
            return;
        };
        let prefix = self
            .syntax(&project)
            .comment_prefix(Path::new(&record.region.file));
        let kept = kept_answer_text(
            &text,
            found.start,
            found.end,
            &reply.text,
            prefix.as_deref(),
        );
        let snapshot = Snapshot {
            root: self.root.clone(),
            open: [(
                std::path::PathBuf::from(&record.region.file),
                OpenDocument {
                    editor_path: path.clone(),
                    version,
                    text: text.clone(),
                },
            )]
            .into(),
        };
        let edits = line_edits(&text, &kept);
        let change = FileChange {
            path: Path::new(&record.region.file),
            new: false,
            before: Some(&text),
            after: &kept,
            edits: &edits,
        };
        let (edit, _) = workspace_edit(&[change], &snapshot, NewFiles::LeftOut);
        self.client
            .apply_edit("apprentice: keep this answer", edit, |_| {});
    }

    /// *Dismiss*: never shown again at this text.
    pub(super) fn dismiss(&self, id: &str) {
        let Some((project, record)) = self.record(id) else {
            return;
        };
        if let Some(regions) = self.region_store(&project) {
            let _ = dismiss_region(&record, regions.as_ref(), self.cfg.clock.as_ref());
        }
        self.publish(&self.root.join(&record.region.file));
    }

    /// A label: what the person says the region was.
    pub(super) fn label(&self, id: &str, kind: Option<Kind>) {
        let Some((project, record)) = self.record(id) else {
            return;
        };
        if let Some(regions) = self.region_store(&project) {
            let _ = label_region(&record, kind, regions.as_ref(), self.cfg.clock.as_ref());
        }
        let said = kind.map_or("nothing", Kind::word);
        self.client.say(
            MessageType::INFO,
            &format!("apprentice: labelled as {said}"),
        );
    }

    /// A long answer, also as a note beside the code.
    pub(super) fn write_note(&self, id: &str, title: &str, text: &str) -> Option<()> {
        let project = self.project()?;
        let notes = StateFiles::for_project(&project).ok()?.notes();
        std::fs::create_dir_all(&notes).ok()?;
        let path = notes.join(format!("{id}.md"));
        std::fs::write(&path, format!("# {}\n\n{text}\n", title.trim())).ok()?;
        self.open_note(&path);
        Some(())
    }

    /// Opens a note beside the code (`sorcerer/note`), else over it.
    pub(super) fn open_note(&self, path: &Path) {
        let uri = sorcerer_lsp::uri_of(path);
        if !self
            .client
            .notify_sorcerer("sorcerer/note", json!({ "uri": uri }))
        {
            self.client.show_document(path, None);
        }
    }
}

/// The file's text with the answer written in under lines `start..=end` (from 1): each line
/// of the answer behind the comment prefix, at the region's indentation; with no prefix, as a
/// paragraph after a blank line.
///
/// Example: under `    # why slow?` with prefix `#`, the answer `It reads twice.` is the line
/// `    # It reads twice.`.
pub(super) fn kept_answer_text(
    text: &str,
    start: u32,
    end: u32,
    answer: &str,
    prefix: Option<&str>,
) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let first = lines.get(start as usize - 1).copied().unwrap_or_default();
    let indent: String = first
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .collect();
    let ending = if first.ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let written: String = match prefix {
        Some(prefix) => answer
            .lines()
            .map(|line| match line.trim() {
                "" => format!("{indent}{prefix}{ending}"),
                _ => format!("{indent}{prefix} {line}{ending}"),
            })
            .collect(),
        None => format!("{ending}{answer}{ending}"),
    };
    let at = (end as usize).min(lines.len());
    let mut kept: String = lines[..at].concat();
    if !kept.ends_with('\n') && !kept.is_empty() {
        kept.push_str(ending);
    }
    kept.push_str(&written);
    kept.push_str(&lines[at..].concat());
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_kept_as_a_comment_at_the_regions_indentation() {
        let text = "def f():\n    # why slow?\n    g()\n";
        assert_eq!(
            kept_answer_text(text, 2, 2, "It reads twice.\nSee g.", Some("#")),
            "def f():\n    # why slow?\n    # It reads twice.\n    # See g.\n    g()\n"
        );
        assert_eq!(
            kept_answer_text("Why?\n", 1, 1, "Because.", None),
            "Why?\n\nBecause.\n"
        );
    }
}
