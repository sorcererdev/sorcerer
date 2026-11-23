//! Applying a plan, the records, and the open holes, on a copy of bistro.

mod support;

use sorcerer_core::adapters::platform::paths::link_to;
use spellbook::domain::plan::FileStatus;
use spellbook::ports::generation_store::GenerationStore;
use spellbook::ports::holes_store::HolesStore;
use spellbook::ports::workspace::WriteRefused;
use support::*;

const MAIN: &str = "src/bistro/booking/commands/cancel_table.py";
const TEST: &str = "src/bistro/booking/commands/cancel_table__test.py";

fn read(root: &std::path::Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

#[test]
fn apply_writes_the_new_files_and_records_the_generation() {
    let dir = bistro_project();
    let rig = PlanRig::new(dir.path());
    let plan = rig.plan("command", &[("command", "cancel_table")]).unwrap();
    let main_text = plan.files[1].text.clone();
    let applied = rig.apply(plan);
    assert_eq!(applied.written, [MAIN, TEST]);
    assert_eq!(applied.skipped, ["src/bistro/booking/commands/__init__.py"]);
    assert_eq!(applied.refused, None);
    assert_eq!(read(dir.path(), MAIN), main_text);
    let records = rig.apply_rig().generations.all().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].generator, "command");
    assert_eq!(records[0].inputs["command"], "cancel_table");
    assert_eq!(records[0].files.len(), 3);
    assert_eq!(records[0].files[0].status, FileStatus::Skipped);
}

#[test]
fn holes_json_holds_the_placeholder_places() {
    let dir = bistro_project();
    let rig = PlanRig::new(dir.path());
    rig.apply(rig.plan("command", &[("command", "cancel_table")]).unwrap());
    let text = read(dir.path(), ".sorcerer/state/spellbook/holes.json");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["version"], 1);
    let rows: Vec<(String, String, u64)> = json["data"]["holes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["name"].as_str().unwrap().to_string(),
                row["file"].as_str().unwrap().to_string(),
                row["line"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("field".to_string(), MAIN.to_string(), 10),
            ("type".to_string(), MAIN.to_string(), 10),
            ("handle".to_string(), MAIN.to_string(), 30),
            ("field".to_string(), TEST.to_string(), 11),
        ]
    );
    assert_eq!(rig.apply_rig().holes.read().unwrap().len(), 4);
}

#[test]
fn a_file_that_appeared_after_the_plan_is_refused() {
    let dir = bistro_project();
    let rig = PlanRig::new(dir.path());
    let plan = rig.plan("command", &[("command", "cancel_table")]).unwrap();
    write_file(dir.path(), MAIN, "mine = 1\n");
    let applied = rig.apply(plan);
    assert_eq!(applied.refused, Some((MAIN.to_string(), WriteRefused::Exists)));
    assert!(applied.written.is_empty());
    assert_eq!(read(dir.path(), MAIN), "mine = 1\n");
    assert!(!dir.path().join(TEST).exists());
    let records = rig.apply_rig().generations.all().unwrap();
    assert_eq!(records[0].files.len(), 1, "only the skipped file: {:?}", records[0].files);
    assert!(records[0].holes.is_empty());
}

#[test]
fn write_new_refuses_a_link_out() {
    let dir = bistro_project();
    let outside = tempfile::tempdir().unwrap();
    link_to(outside.path(), &dir.path().join("src/link")).unwrap();
    write_generator(
        dir.path(),
        "tiny",
        "name = \"Tiny\"\nlanguage = \"python\"\n[inputs.name]\ntype = \"ident\"\n[[edits]]\ncreate = \"src/link/{= name =}.py\"\ntext = \"x = 1\\n\"\n",
        &[],
    );
    let rig = PlanRig::new(dir.path());
    let plan = rig.plan("tiny", &[("name", "x")]).unwrap();
    let applied = rig.apply(plan);
    assert_eq!(applied.refused, Some(("src/link/x.py".to_string(), WriteRefused::Fenced)));
    assert!(!outside.path().join("x.py").exists());
}

#[test]
fn folders_are_created() {
    let dir = bistro_project();
    write_generator(
        dir.path(),
        "tiny",
        "name = \"Tiny\"\nlanguage = \"python\"\n[inputs.name]\ntype = \"ident\"\n[[edits]]\ncreate = \"src/new/deep/{= name =}.py\"\ntext = \"x = 1\\n\"\n",
        &[],
    );
    let rig = PlanRig::new(dir.path());
    let applied = rig.apply(rig.plan("tiny", &[("name", "x")]).unwrap());
    assert_eq!(applied.written, ["src/new/deep/x.py"]);
    assert_eq!(read(dir.path(), "src/new/deep/x.py"), "x = 1\n");
}

#[test]
fn the_generation_record_is_one_versioned_line() {
    let dir = bistro_project();
    let rig = PlanRig::new(dir.path());
    let plan = rig.plan("command", &[("command", "cancel_table")]).unwrap();
    let id = plan.id.clone();
    rig.apply(plan);
    let text = read(dir.path(), ".sorcerer/state/spellbook/generations.jsonl");
    assert_eq!(text.lines().count(), 1);
    let line: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(line["version"], 1);
    assert_eq!(line["id"], id.as_str());
    assert_eq!(line["files"][1]["status"], "new");
    assert_eq!(line["holes"][0]["source"], "derived");
    assert_eq!(line["at"], "2026-11-22T12:00:00Z");
}
