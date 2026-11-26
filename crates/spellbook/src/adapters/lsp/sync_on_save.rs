//! Owned regions kept as the person saves: a save that makes a region's block stale (a file
//! written, a file deleted) is followed by the sync, a region file the editor has open changed
//! through one edit the person can undo (tied to the version the editor last reported), any
//! other written to disk, each said in a line. A sync applied from the editor's plan
//! (`spellbook.apply`) follows the same split (`split_sync`).
//! `[spellbook] sync_on_save = false` leaves the blocks for the `region-stale` finding.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_types::MessageType;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::ports::view::CodebaseView;

use super::project_paths::project_relative;
use super::{Server, lock};
use crate::adapters::wiring::Wired;
use crate::domain::config::spellbook_config;
use crate::domain::plan::{Plan, PlanStop, PlannedFile, plan_stop_text};
use crate::ports::workspace::write_refused_text;
use crate::usecases::apply_plan::{AppliedGeneration, ChangesLanded, apply_plan};
use crate::usecases::plan_generation::PlanDeps;
use crate::usecases::planned_view::PlannedView;
use crate::usecases::sync_regions::{SyncMode, instances, plan_sync};

/// A sync planned over the project as the editor has it, and the version of each open document
/// it read, by its path in the plan.
pub(super) struct EditorSync {
    pub(super) plan: Plan,
    pub(super) versions: BTreeMap<String, i32>,
}

/// A sync's files by who lands them: the editor (open, with the version the plan read), the
/// disk (not open), or nobody (open, but with no version known: left for its finding).
pub(super) struct SyncSplit {
    pub(super) in_editor: Plan,
    pub(super) on_disk: Plan,
    pub(super) left_stale: Plan,
}

impl Server {
    /// Syncs the regions after a save: (1) nothing where spellbook may not run (not trusted,
    /// turned off); (2) the sync planned over the files as the editor has them (an open file's
    /// text and version as the editor last reported them); (3) a plan that cannot be made
    /// (broken markers, a `regions.toml` that does not load) is said once; (4) with
    /// `sync_on_save` off, the blocks out of date are said once and left for their findings;
    /// (5) otherwise each region file the editor has open lands as an edit tied to its version,
    /// each other one is written to disk, and an open one whose version is not known is left
    /// for its finding.
    ///
    /// Example: saving a new `commands/close_table.rs` in bistro-rs, with `commands/mod.rs` not
    /// open, writes `pub mod close_table;` into its block and says `spellbook: synced region
    /// modules in src/booking/commands/mod.rs (+1 −0)`.
    pub(super) fn sync_on_save(self: &Arc<Self>) {
        let Ok(wired) = self.wired_for_commands() else {
            return;
        };
        let sync = match self.sync_planned(&wired, &SyncMode::Refresh) {
            Ok(sync) => sync,
            Err(stop) => return self.say_regions_cannot_sync(&stop),
        };
        if sync.plan.files.is_empty() {
            return;
        }
        if !sync_on_save_wanted(&wired) {
            return self.say_out_of_date(sync.plan.files.len());
        }
        let split = self.split_sync(sync.plan, &sync.versions);
        self.say_left_stale(&split.left_stale);
        self.say_written(self.write_region_files(&wired, split.on_disk));
        self.land_in_open_files(split.in_editor, sync.versions);
    }

    /// A sync planned over the project as the editor has it: a file the editor has open is read
    /// from the text it last reported (unsaved changes included), and a file a fix or a sync
    /// landed in and the person has not saved from the text the plan left in it, so a block is
    /// never planned twice nor against text the buffer no longer holds.
    pub(super) fn sync_planned(
        &self,
        wired: &Wired,
        mode: &SyncMode,
    ) -> Result<EditorSync, PlanStop> {
        let documents = self.open_documents(wired);
        let mut texts = self.landing_texts();
        texts.extend(
            documents
                .iter()
                .map(|(file, (_, text))| (file.clone(), text.clone())),
        );
        let view = PlannedView::new(&wired.view, texts);
        let deps = PlanDeps {
            view: &view,
            ..wired.plan_deps()
        };
        let plan = plan_sync(&deps, mode)?.plan;
        let versions = plan
            .files
            .iter()
            .filter_map(|file| {
                documents
                    .get(&file.path)
                    .map(|(v, _)| (file.path.clone(), *v))
            })
            .collect();
        Ok(EditorSync { plan, versions })
    }

    /// The project's files the editor has open and the disk still holds, by their path in a
    /// plan: the version and text it last reported. A buffer whose file was deleted is left out,
    /// so the deleted file is not listed by a region.
    fn open_documents(&self, wired: &Wired) -> BTreeMap<String, (i32, String)> {
        self.client
            .open_paths()
            .into_iter()
            .filter_map(|path| {
                let document = self.client.text_of(&path)?;
                let file = project_relative(&wired.project, &path)?;
                wired
                    .view
                    .exists(&file)
                    .then(|| (forward_slashes(&file), document))
            })
            .collect()
    }

    /// Publishes again the marks of every open file a region keeps, other than `saved`: a file
    /// written or deleted can make another file's block stale or fresh.
    pub(super) fn republish_region_files(&self, saved: &Path) {
        let Ok(wired) = self.wired_for_commands() else {
            return;
        };
        let Ok(regions) = &wired.regions else {
            return;
        };
        let region_files: BTreeSet<String> = instances(&wired.view, regions)
            .into_iter()
            .map(|instance| instance.file)
            .collect();
        let open: Vec<PathBuf> = lock(&self.published)
            .keys()
            .filter(|path| path.as_path() != saved)
            .filter(|path| {
                project_relative(&wired.project, path)
                    .is_some_and(|file| region_files.contains(&forward_slashes(&file)))
            })
            .cloned()
            .collect();
        for path in open {
            self.republish(&path);
        }
    }

    /// Who lands each file of a sync: the editor lands a file it has open whose version the plan
    /// read (so the person can undo, and the editor refuses a buffer that moved on); a file not
    /// open is written to disk; an open file with no version is not touched. An editor that
    /// cannot apply edits has every file written to disk.
    pub(super) fn split_sync(&self, plan: Plan, versions: &BTreeMap<String, i32>) -> SyncSplit {
        let editor_lands = self.client.can_apply_edits();
        let mut split = SyncSplit {
            in_editor: with_files(&plan, Vec::new()),
            on_disk: with_files(&plan, Vec::new()),
            left_stale: with_files(&plan, Vec::new()),
        };
        for file in plan.files {
            let open = editor_lands && self.client.is_open(&self.root.join(&file.path));
            let lander = match (open, versions.contains_key(&file.path)) {
                (false, _) => &mut split.on_disk,
                (true, true) => &mut split.in_editor,
                (true, false) => &mut split.left_stale,
            };
            lander.files.push(file);
        }
        split
    }

    /// Writes the region files the editor does not have open, each only onto the text the plan
    /// saw; `None` for no files, or a plan that could not be recorded (logged).
    pub(super) fn write_region_files(
        &self,
        wired: &Wired,
        plan: Plan,
    ) -> Option<AppliedGeneration> {
        if plan.files.is_empty() {
            return None;
        }
        match apply_plan(&wired.apply_deps(), plan, ChangesLanded::NotYet) {
            Ok(applied) => Some(applied),
            Err(e) => {
                self.client
                    .log(&format!("spellbook: the sync was not written: {e:#}"));
                None
            }
        }
    }

    /// Says each file written to disk synced, or why it was not.
    fn say_written(&self, applied: Option<AppliedGeneration>) {
        let Some(applied) = applied else {
            return;
        };
        for file in &applied.plan.files {
            if applied.changed.contains(&file.path) {
                self.say_line(&synced_text(file));
            } else if let Some((_, why)) = &applied.refused {
                self.say_line(&not_synced_text(file, &write_refused_text(why)));
            }
        }
    }

    /// Says each open file the sync left alone because its version is not known.
    fn say_left_stale(&self, plan: &Plan) {
        for file in &plan.files {
            self.say_line(&not_synced_text(
                file,
                "its version in the editor is not known",
            ));
        }
    }

    /// Sends the region files the editor has open as one edit, each tied to the version the plan
    /// read; once it is applied the sync is recorded and each file said; refused (the buffer
    /// moved on), each file is said not synced, and its finding stays.
    fn land_in_open_files(self: &Arc<Self>, plan: Plan, versions: BTreeMap<String, i32>) {
        if plan.files.is_empty() {
            return;
        }
        self.send_edit(plan, versions, |server, plan, applied| {
            if !applied {
                for file in &plan.files {
                    server.say_line(&not_synced_text(file, "the editor refused the edit"));
                }
                return;
            }
            let paths: Vec<PathBuf> = plan
                .files
                .iter()
                .map(|f| server.root.join(&f.path))
                .collect();
            for file in &plan.files {
                server.say_line(&synced_text(file));
            }
            server.record_landed_sync(plan);
            for path in paths {
                server.republish(&path);
            }
        });
    }

    /// The record of a sync the editor landed; the files are the editor's buffers to save.
    pub(super) fn record_landed_sync(&self, plan: Plan) -> Vec<String> {
        let recorded = self.wired_for_commands().and_then(|wired| {
            apply_plan(&wired.apply_deps(), plan, ChangesLanded::ByTheEditor)
                .map(|applied| applied.changed)
                .map_err(|e| format!("{e:#}"))
        });
        recorded.unwrap_or_else(|why| {
            self.client
                .log(&format!("spellbook: the sync was not recorded: {why}"));
            Vec::new()
        })
    }

    fn say_regions_cannot_sync(&self, stop: &PlanStop) {
        self.client.say_once(
            "regions-cannot-sync",
            MessageType::WARNING,
            &format!("spellbook: {}", plan_stop_text(stop)),
        );
    }

    fn say_out_of_date(&self, blocks: usize) {
        self.client.say_once(
            "regions-out-of-date",
            MessageType::INFO,
            &out_of_date_text(blocks),
        );
    }

    fn say_line(&self, text: &str) {
        self.client.say(MessageType::INFO, text);
    }
}

/// Whether the project wants its regions synced on save (`[spellbook] sync_on_save`, on by
/// default).
fn sync_on_save_wanted(wired: &Wired) -> bool {
    spellbook_config(&wired.project).is_ok_and(|config| config.sync_on_save)
}

/// The plan with only these of its files.
fn with_files(plan: &Plan, files: Vec<PlannedFile>) -> Plan {
    Plan {
        files,
        ..plan.clone()
    }
}

/// `spellbook: synced region modules in src/booking/commands/mod.rs (+1 −0)`.
fn synced_text(file: &PlannedFile) -> String {
    format!(
        "spellbook: synced region {} in {} (+{} \u{2212}{})",
        region_of(file),
        file.path,
        file.added,
        file.removed
    )
}

/// `spellbook: region modules in src/booking/commands/mod.rs was not synced (the editor
/// refused the edit); run spellbook sync`.
fn not_synced_text(file: &PlannedFile, why: &str) -> String {
    format!(
        "spellbook: region {} in {} was not synced ({why}); run spellbook sync",
        region_of(file),
        file.path
    )
}

/// `spellbook: 1 region out of date; run spellbook sync`.
fn out_of_date_text(blocks: usize) -> String {
    let noun = if blocks == 1 { "region" } else { "regions" };
    format!("spellbook: {blocks} {noun} out of date; run spellbook sync")
}

/// The region (or regions, `a, b`) a file of a sync rewrites.
fn region_of(file: &PlannedFile) -> &str {
    file.region.as_deref().unwrap_or_default()
}
