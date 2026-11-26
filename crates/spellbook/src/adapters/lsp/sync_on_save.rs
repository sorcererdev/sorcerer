//! Owned regions kept as the person saves: a save that makes a region's block stale (a file
//! written, a file deleted) is followed by the sync, a region file the editor has open changed
//! through one edit the person can undo, any other written to disk, each said in a line.
//! `[spellbook] sync_on_save = false` leaves the blocks for the `region-stale` finding.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_types::MessageType;
use sorcerer_core::domain::paths::forward_slashes;

use super::project_paths::project_relative;
use super::{Server, lock};
use crate::adapters::wiring::Wired;
use crate::domain::config::spellbook_config;
use crate::domain::plan::{Plan, PlanStop, PlannedFile, plan_stop_text};
use crate::ports::workspace::write_refused_text;
use crate::usecases::apply_plan::{ChangesLanded, apply_plan};
use crate::usecases::plan_generation::PlanDeps;
use crate::usecases::planned_view::PlannedView;
use crate::usecases::sync_regions::{SyncMode, instances, plan_sync};

impl Server {
    /// Syncs the regions after a save: (1) nothing where spellbook may not run (not trusted,
    /// turned off); (2) the sync planned over the files as the editor has them, a sync landed
    /// and not saved yet read from its planned text; (3) a plan that cannot be made (broken
    /// markers, a `regions.toml` that does not load) is said once; (4) with `sync_on_save` off,
    /// the blocks out of date are said once and left for their findings; (5) otherwise each
    /// region file the editor has open lands as an edit, and each other one is written to disk.
    ///
    /// Example: saving a new `commands/close_table.rs` in bistro-rs, with `commands/mod.rs` not
    /// open, writes `pub mod close_table;` into its block and says `spellbook: synced region
    /// modules in src/booking/commands/mod.rs (+1 −0)`.
    pub(super) fn sync_on_save(self: &Arc<Self>) {
        let Ok(wired) = self.wired_for_commands() else {
            return;
        };
        let plan = match self.sync_planned(&wired, &SyncMode::Refresh) {
            Ok(plan) => plan,
            Err(stop) => return self.say_regions_cannot_sync(&stop),
        };
        if plan.files.is_empty() {
            return;
        }
        if !sync_on_save_wanted(&wired) {
            return self.say_out_of_date(plan.files.len());
        }
        let (open, closed) = self.open_and_closed(plan);
        self.write_to_disk(&wired, closed);
        self.land_in_open_files(open);
    }

    /// A sync planned over the project as the editor has it: a file a fix or a sync landed in
    /// and the person has not saved is read from the text the plan left in it, so its block is
    /// not planned a second time.
    pub(super) fn sync_planned(&self, wired: &Wired, mode: &SyncMode) -> Result<Plan, PlanStop> {
        let view = PlannedView::new(&wired.view, self.landing_texts());
        let deps = PlanDeps {
            view: &view,
            ..wired.plan_deps()
        };
        plan_sync(&deps, mode).map(|planned| planned.plan)
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

    /// The sync split in two: the files the editor has open (it lands them, so the person can
    /// undo), and the rest. An editor that cannot apply edits has every file written to disk.
    fn open_and_closed(&self, plan: Plan) -> (Plan, Plan) {
        let editor_lands = self.client.can_apply_edits();
        let (open, closed): (Vec<PlannedFile>, Vec<PlannedFile>) = plan
            .files
            .iter()
            .cloned()
            .partition(|file| editor_lands && self.client.is_open(&self.root.join(&file.path)));
        (with_files(&plan, open), with_files(&plan, closed))
    }

    /// Writes the region files the editor does not have open, each only onto the text the plan
    /// saw, and says each one synced or why it was not.
    fn write_to_disk(&self, wired: &Wired, plan: Plan) {
        if plan.files.is_empty() {
            return;
        }
        let applied = match apply_plan(&wired.apply_deps(), plan, ChangesLanded::NotYet) {
            Ok(applied) => applied,
            Err(e) => return self.say_line(&format!("spellbook: {e:#}")),
        };
        for file in &applied.plan.files {
            if applied.changed.contains(&file.path) {
                self.say_line(&synced_text(file));
            } else if let Some((_, why)) = &applied.refused {
                self.say_line(&not_synced_text(file, &write_refused_text(why)));
            }
        }
    }

    /// Sends the region files the editor has open as one edit; once it is applied the sync is
    /// recorded and each file said; refused, each file is said not synced, and its finding
    /// stays.
    fn land_in_open_files(self: &Arc<Self>, plan: Plan) {
        if plan.files.is_empty() {
            return;
        }
        self.send_edit(plan, None, |server, plan, applied| {
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
            // the record of the sync; the files are the editor's buffers to save
            let recorded = server.wired_for_commands().and_then(|wired| {
                apply_plan(&wired.apply_deps(), plan, ChangesLanded::ByTheEditor)
                    .map(|_| ())
                    .map_err(|e| format!("{e:#}"))
            });
            if let Err(why) = recorded {
                server
                    .client
                    .log(&format!("spellbook: the sync was not recorded: {why}"));
            }
            for path in paths {
                server.republish(&path);
            }
        });
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
