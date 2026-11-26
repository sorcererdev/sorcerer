//! Owned regions on a copy of bistro-rs: set up, refreshed, found stale, refreshed by `new`.

mod support;

use std::path::Path;

use spellbook::domain::plan::{FileStatus, plan_stop_text};
use spellbook::usecases::sync_regions::{SyncMode, SyncPlan, plan_sync, region_findings};
use support::*;
use tempfile::TempDir;

fn bistro_rs() -> TempDir {
    bistro_rs_project()
}

fn sync(root: &Path, mode: SyncMode) -> SyncPlan {
    let rig = PlanRig::new(root);
    plan_sync(&rig.deps(), &mode).unwrap_or_else(|stop| panic!("{}", plan_stop_text(&stop)))
}

fn set_up_everything(root: &Path) {
    for name in ["modules", "ports"] {
        let planned = sync(root, SyncMode::Init(name.into()));
        PlanRig::new(root).apply(planned.plan);
    }
}

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap()
}

#[test]
fn nothing_set_up_is_said_in_the_notes() {
    let project = bistro_rs();
    let planned = sync(project.path(), SyncMode::Refresh);
    assert!(planned.plan.files.is_empty());
    assert_eq!(planned.blocks, 0);
    assert_eq!(
        planned.plan.notes,
        vec![
            "region modules is not set up in src/booking/commands/mod.rs, src/booking/models/mod.rs, src/booking/queries/mod.rs, src/web/bookings/mod.rs  (run spellbook sync --init modules)".to_string(),
            "region ports is not set up in src/booking/ports/mod.rs  (run spellbook sync --init ports)".to_string(),
        ]
    );
}

#[test]
fn init_puts_the_markers_after_the_hand_lines_and_adopts_them() {
    let project = bistro_rs();
    let planned = sync(project.path(), SyncMode::Init("modules".into()));
    let rows: Vec<(&str, Option<u32>)> = planned
        .plan
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.set_up))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("src/booking/commands/mod.rs", Some(1)),
            ("src/booking/models/mod.rs", Some(3)),
            ("src/booking/queries/mod.rs", Some(1)),
            ("src/web/bookings/mod.rs", Some(3)),
        ]
    );
    PlanRig::new(project.path()).apply(planned.plan);
    assert_eq!(
        read(project.path(), "src/booking/commands/mod.rs"),
        "//! Commands: each changes the bookings, and holds the booking rules for its change.\n\n\
         // spellbook:region modules (from src/booking/commands/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)\n\
         pub mod create_booking;\n\
         // spellbook:end modules\n"
    );
}

#[test]
fn ports_are_listed_by_their_folders() {
    let project = bistro_rs();
    let planned = sync(project.path(), SyncMode::Init("ports".into()));
    PlanRig::new(project.path()).apply(planned.plan);
    let text = read(project.path(), "src/booking/ports/mod.rs");
    assert!(
        text.ends_with(
            "// spellbook:region ports (from src/booking/ports/*/mod.rs; edit .sorcerer/spellbook/regions.toml, not this block)\n\
             pub mod booking_repository;\n\
             pub mod id_generator;\n\
             // spellbook:end ports\n"
        ),
        "{text}"
    );
}

#[test]
fn once_set_up_a_sync_has_nothing_to_do() {
    let project = bistro_rs();
    set_up_everything(project.path());
    let planned = sync(project.path(), SyncMode::Refresh);
    assert!(planned.plan.files.is_empty(), "{:#?}", planned.plan.files);
    assert!(planned.plan.notes.is_empty());
    assert_eq!(planned.blocks, 5);
}

#[test]
fn a_deleted_file_is_a_stale_region_and_sync_removes_its_line() {
    let project = bistro_rs();
    set_up_everything(project.path());
    std::fs::remove_file(
        project
            .path()
            .join("src/booking/queries/get_day_schedule.rs"),
    )
    .unwrap();
    let rig = PlanRig::new(project.path());
    let regions = rig.regions.as_ref().unwrap();
    let found = region_findings(&rig.view, &rig.grammars, regions, None).unwrap();
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].rule, "region-stale");
    assert_eq!(found[0].file, "src/booking/queries/mod.rs");
    assert_eq!(found[0].line, 3);
    assert_eq!(
        found[0].message,
        "region modules is out of date (+0 \u{2212}1)"
    );
    let planned = sync(project.path(), SyncMode::Refresh);
    assert_eq!(planned.plan.files.len(), 1);
    assert_eq!(planned.plan.files[0].removed, 1);
    assert_eq!(planned.plan.files[0].region.as_deref(), Some("modules"));
}

#[test]
fn a_new_command_adds_its_line_in_the_same_plan() {
    let project = bistro_rs();
    set_up_everything(project.path());
    let plan = PlanRig::new(project.path())
        .plan(
            "command",
            &[("service", "booking"), ("command", "cancel_table")],
        )
        .unwrap_or_else(|stop| panic!("{}", plan_stop_text(&stop)));
    let module = plan
        .files
        .iter()
        .find(|file| file.path == "src/booking/commands/mod.rs")
        .expect("mod.rs is in the plan");
    assert_eq!(module.status, FileStatus::Changed);
    assert_eq!(module.region.as_deref(), Some("modules"));
    assert_eq!((module.added, module.removed), (1, 0));
    assert!(
        module
            .text
            .contains("pub mod cancel_table;\npub mod create_booking;\n")
    );
}

#[test]
fn a_broken_marker_stops_the_plan_naming_the_line() {
    let project = bistro_rs();
    set_up_everything(project.path());
    let path = project.path().join("src/booking/queries/mod.rs");
    let text = read(project.path(), "src/booking/queries/mod.rs")
        .replace("// spellbook:end modules\n", "");
    std::fs::write(&path, text).unwrap();
    let rig = PlanRig::new(project.path());
    let stop = plan_sync(&rig.deps(), &SyncMode::Refresh).unwrap_err();
    assert_eq!(
        plan_stop_text(&stop),
        "src/booking/queries/mod.rs:3: region modules opens on line 3 and never ends (no `// spellbook:end modules`); nothing was written (fix the markers by hand)"
    );
    let new = rig
        .plan(
            "command",
            &[("service", "booking"), ("command", "cancel_table")],
        )
        .unwrap_err();
    assert!(plan_stop_text(&new).starts_with("src/booking/queries/mod.rs:3"));
}
