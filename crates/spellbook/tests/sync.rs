//! `spellbook sync` and the region findings of `check`, on the command line, on a copy of
//! bistro-rs with the vendored grammars and a fixed clock.

mod support;

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::domain::project::Project;
use spellbook::adapters::wiring::{Wired, wired};
use spellbook::cli::main_with;
use support::*;

/// Runs a command in a directory: the exit code, what it printed and what it said.
fn run(dir: &Path, rest: &[&str]) -> (i32, String, String) {
    let env = MapEnv::default().with_user_dirs(dir);
    let wire = |mut project: Project| -> sorcerer_core::Result<Wired> {
        point_at_vendored(&mut project.languages);
        wired(project, test_cache(), Arc::new(FixedClock::at_noon()))
    };
    let args: Vec<OsString> = std::iter::once("spellbook")
        .chain(rest.iter().copied())
        .map(OsString::from)
        .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_with(args, dir, &env, &wire, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn set_up(dir: &Path) {
    for name in ["modules", "ports"] {
        let (code, _, err) = run(dir, &["sync", "--init", name]);
        assert_eq!(code, 0, "{err}");
    }
}

#[test]
fn check_says_what_is_not_set_up_and_exits_one() {
    let dir = bistro_rs_project();
    let (code, out, _) = run(dir.path(), &["sync", "--check"]);
    assert_eq!(code, 1);
    assert_eq!(
        out,
        "region modules is not set up in src/booking/commands/mod.rs, src/booking/models/mod.rs, src/booking/queries/mod.rs, src/web/bookings/mod.rs  (run spellbook sync --init modules)\n\
         region ports is not set up in src/booking/ports/mod.rs  (run spellbook sync --init ports)\n\
         nothing written (--check)\n"
    );
}

#[test]
fn init_sets_up_every_file_and_a_second_sync_is_up_to_date() {
    let dir = bistro_rs_project();
    let (code, out, err) = run(dir.path(), &["sync", "--init", "modules"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "sync (4 files changed)\n\
         \x20 ~ src/booking/commands/mod.rs  (region modules: set up, 1 item)\n\
         \x20 ~ src/booking/models/mod.rs    (region modules: set up, 3 items)\n\
         \x20 ~ src/booking/queries/mod.rs   (region modules: set up, 1 item)\n\
         \x20 ~ src/web/bookings/mod.rs      (region modules: set up, 3 items)\n\
         region ports is not set up in src/booking/ports/mod.rs  (run spellbook sync --init ports)\n"
    );
    let (code, out, _) = run(dir.path(), &["sync", "--init", "ports"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "sync (1 file changed)\n  ~ src/booking/ports/mod.rs  (region ports: set up, 2 items)\n"
    );
    let (code, out, _) = run(dir.path(), &["sync"]);
    assert_eq!(code, 0);
    assert_eq!(out, "regions up to date (5 regions)\n");
    let (code, _, _) = run(dir.path(), &["sync", "--check"]);
    assert_eq!(code, 0);
}

#[test]
fn a_deleted_file_is_a_finding_with_the_sync_fix() {
    let dir = bistro_rs_project();
    set_up(dir.path());
    std::fs::remove_file(dir.path().join("src/booking/queries/get_day_schedule.rs")).unwrap();
    let (code, out, _) = run(dir.path(), &["check"]);
    assert_eq!(code, 1);
    assert_eq!(
        out,
        "✗ src/booking/queries/mod.rs:3  region-stale — region modules is out of date (+0 \u{2212}1)  (fixes: Sync region modules)\n\
         1 blocking finding\n"
    );
    let (_, json, _) = run(dir.path(), &["check", "--json"]);
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    let finding = &report["findings"][0];
    assert_eq!(finding["rule"], "region-stale");
    assert_eq!(finding["region"], "modules");
    assert_eq!(finding["added"], 0);
    assert_eq!(finding["removed"], 1);
    assert_eq!(finding["fixes"][0]["generator"], "sync");
    assert_eq!(finding["fixes"][0]["label"], "Sync region modules");
    assert_eq!(
        finding["fixes"][0]["inputs"]["file"],
        "src/booking/queries/mod.rs"
    );
    let (code, out, _) = run(dir.path(), &["sync", "--check"]);
    assert_eq!(code, 1);
    assert!(out.starts_with(
        "region modules is out of date in src/booking/queries/mod.rs (+0 \u{2212}1)  (run spellbook sync)\n"
    ));
    let (code, out, _) = run(dir.path(), &["sync"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "sync (1 file changed)\n  ~ src/booking/queries/mod.rs  (region modules: +0 \u{2212}1)\n"
    );
    let (code, out, _) = run(dir.path(), &["check"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "conventions hold (no findings)\n")
    );
}

#[test]
fn plan_shows_the_hunks_and_writes_nothing() {
    let dir = bistro_rs_project();
    set_up(dir.path());
    std::fs::write(
        dir.path().join("src/booking/commands/seat_walk_in.rs"),
        "pub fn seat() {}\n",
    )
    .unwrap();
    let before = std::fs::read_to_string(dir.path().join("src/booking/commands/mod.rs")).unwrap();
    let (code, out, _) = run(dir.path(), &["sync", "--plan"]);
    assert_eq!(code, 0);
    assert!(out.contains("+pub mod seat_walk_in;\n"), "{out}");
    assert!(out.ends_with("nothing written (--plan)\n"));
    let after = std::fs::read_to_string(dir.path().join("src/booking/commands/mod.rs")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn a_new_command_lists_its_module_in_the_same_plan() {
    let dir = bistro_rs_project();
    set_up(dir.path());
    let (code, out, err) = run(
        dir.path(),
        &[
            "new",
            "command",
            "--set",
            "service=booking",
            "--set",
            "command=cancel_table",
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("command (1 new, 1 changed, "), "{out}");
    assert!(
        out.contains("~ src/booking/commands/mod.rs           (region modules: +1 \u{2212}0)"),
        "{out}"
    );
    let text = std::fs::read_to_string(dir.path().join("src/booking/commands/mod.rs")).unwrap();
    assert!(text.contains("pub mod cancel_table;\npub mod create_booking;\n"));
}

#[test]
fn a_region_not_set_up_is_a_note_on_new() {
    let dir = bistro_rs_project();
    let (code, out, _) = run(
        dir.path(),
        &[
            "new",
            "command",
            "--set",
            "service=booking",
            "--set",
            "command=cancel_table",
        ],
    );
    assert_eq!(code, 0);
    assert!(out.contains("region modules is not set up in src/booking/commands/mod.rs"));
}

#[test]
fn what_cannot_run_exits_two() {
    let dir = bistro_rs_project();
    let (code, _, err) = run(dir.path(), &["sync", "--init", "modulez"]);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        "spellbook: no region named modulez (the regions are: modules, ports)\n"
    );
    std::fs::write(
        dir.path().join(".sorcerer/spellbook/regions.toml"),
        "[regions.m]\nfile = \"a.rs\"\nquery = { glob = \"*\" }\nitme = \"x\"\n",
    )
    .unwrap();
    let (code, _, err) = run(dir.path(), &["sync"]);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        "spellbook: .sorcerer/spellbook/regions.toml: region m: `itme` is not a key; did you mean `item`?\n"
    );
    let (code, _, err) = run(dir.path(), &["check"]);
    assert_eq!(code, 2, "{err}");
    let (code, out, _) = run(
        dir.path(),
        &[
            "new",
            "command",
            "--set",
            "service=booking",
            "--set",
            "command=cancel_table",
        ],
    );
    assert_eq!(code, 0);
    assert!(out.contains(
        "regions are not being refreshed: .sorcerer/spellbook/regions.toml: region m: `itme`"
    ));
}

#[test]
fn a_broken_marker_stops_sync_with_exit_one() {
    let dir = bistro_rs_project();
    set_up(dir.path());
    let path = dir.path().join("src/booking/queries/mod.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("// spellbook:end modules\n", "");
    std::fs::write(&path, text).unwrap();
    let (code, _, err) = run(dir.path(), &["sync"]);
    assert_eq!(code, 1);
    assert!(
        err.starts_with("spellbook: src/booking/queries/mod.rs:3: region modules opens on line 3")
    );
}

#[test]
fn doctor_says_what_the_regions_keep() {
    let dir = bistro_rs_project();
    run(dir.path(), &["sync", "--init", "modules"]);
    let (_, out, _) = run(dir.path(), &["doctor"]);
    let row = out
        .lines()
        .find(|line| line.starts_with("regions"))
        .expect("a regions row");
    assert_eq!(
        row,
        "regions      ok    2 declared, 5 files, 1 not set up (ports in src/booking/ports/mod.rs)"
    );
}

#[test]
fn fix_on_a_region_finding_runs_the_sync() {
    let dir = bistro_rs_project();
    set_up(dir.path());
    std::fs::remove_file(dir.path().join("src/booking/queries/get_day_schedule.rs")).unwrap();
    let (code, out, err) = run(dir.path(), &["fix", "src/booking/queries/mod.rs:3"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "sync (1 file changed)\n  ~ src/booking/queries/mod.rs  (region modules: +0 \u{2212}1)\n"
    );
}
