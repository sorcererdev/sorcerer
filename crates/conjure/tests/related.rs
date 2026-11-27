//! Related files, the language tables and the path templates, on the vendored grammars.

mod support;

use std::path::Path;

use conjure::adapters::imports::related_files;
use conjure::domain::language_tables::{test_table, tests_path};
use sorcerer_core::domain::templates::fill;
use sorcerer_core::ports::view::CodebaseView;
use support::*;

fn related(dir: &Path, lang: &str, file: &str) -> Vec<(String, String)> {
    let (project, view) = project_with_vendored_grammars(dir);
    let loaded = project.languages.load(lang, &test_cache()).unwrap();
    let text = view.text(Path::new(file)).unwrap();
    related_files(&view, &loaded, Path::new(file), &text, &|_| true)
        .unwrap()
        .files
        .into_iter()
        .map(|file| (file.path.to_string_lossy().replace('\\', "/"), file.text))
        .collect()
}

#[test]
fn a_python_file_relates_to_what_it_imports() {
    let d = git_project();
    write(
        d.path(),
        "src/bistro/booking/models/booking.py",
        "class Booking:\n    pass\n",
    );
    write(
        d.path(),
        "src/bistro/booking/commands/cancel.py",
        "import os\nfrom bistro.booking.models.booking import Booking\n\n\ndef f():\n    pass\n",
    );
    let found = related(d.path(), "python", "src/bistro/booking/commands/cancel.py");
    let names: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(names, ["src/bistro/booking/models/booking.py"]);
    assert!(found[0].1.contains("class Booking"));
}

#[test]
fn a_rust_file_relates_to_its_crate_uses() {
    let d = git_project();
    write(d.path(), "src/booking/models/slot.rs", "pub struct Slot;\n");
    write(
        d.path(),
        "src/booking/commands.rs",
        "use crate::booking::models::slot::Slot;\nuse std::fmt;\n\nfn f() {}\n",
    );
    let found = related(d.path(), "rust", "src/booking/commands.rs");
    let names: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(names, ["src/booking/models/slot.rs"]);
}

#[test]
fn a_rust_use_list_and_a_glob_and_an_alias_are_read_too() {
    let d = git_project();
    for m in ["a", "b", "c"] {
        write(d.path(), &format!("src/{m}.rs"), "pub fn x() {}\n");
    }
    write(
        d.path(),
        "src/main.rs",
        "use crate::a::{x, y};\nuse crate::b::*;\nuse crate::c::x as z;\n",
    );
    let found = related(d.path(), "rust", "src/main.rs");
    let names: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(names, ["src/a.rs", "src/b.rs", "src/c.rs"]);
}

#[test]
fn related_files_stop_at_eight_and_120_lines() {
    let d = git_project();
    let mut imports = String::new();
    for i in 0..10 {
        imports.push_str(&format!("import m{i}\n"));
        write(d.path(), &format!("m{i}.py"), &"x = 1\n".repeat(200));
    }
    write(d.path(), "main.py", &imports);
    let found = related(d.path(), "python", "main.py");
    assert_eq!(found.len(), 8);
    assert!(found.iter().all(|(_, t)| t.lines().count() == 120));
    let (project, view) = project_with_vendored_grammars(d.path());
    let loaded = project.languages.load("python", &test_cache()).unwrap();
    let all = related_files(&view, &loaded, Path::new("main.py"), &imports, &|_| true).unwrap();
    assert!(all.files.iter().all(|file| file.lines_in_all == 200));
}

#[test]
fn a_language_whose_imports_query_finds_nothing_relates_nothing() {
    let d = git_project();
    write(
        d.path(),
        ".sorcerer/languages/python/queries/imports.scm",
        "; this project's python names no imports\n",
    );
    write(d.path(), "m.py", "x = 1\n");
    write(d.path(), "main.py", "import m\n");
    assert!(related(d.path(), "python", "main.py").is_empty());
}

#[test]
fn an_unknown_key_in_test_is_refused_by_name() {
    let d = git_project();
    write(
        d.path(),
        ".sorcerer/languages/python/language.toml",
        "[test]\ntests_fro = \"x\"\n",
    );
    let (project, _) = project_with_vendored_grammars(d.path());
    let lang = project.languages.get("python").unwrap();
    let e = test_table(lang).unwrap_err().to_string();
    assert!(
        e.contains("tests_fro") && e.contains("python") && e.contains("[test]"),
        "{e}"
    );
}

#[test]
fn tests_path_fills_dir_and_stem() {
    let d = git_project();
    let (project, _) = project_with_vendored_grammars(d.path());
    let py = project.languages.get("python").unwrap();
    assert_eq!(
        tests_path(py, Path::new("src/a/b.py")).unwrap(),
        Some("src/a/b__test.py".into())
    );
    let rs = project.languages.get("rust").unwrap();
    assert_eq!(
        tests_path(rs, Path::new("src/a/b.rs")).unwrap(),
        Some("src/a/b.rs".into())
    );
    assert_eq!(
        fill("{= dir =}/{=stem=}.x", &[("dir", "d"), ("stem", "s")]),
        Ok("d/s.x".into())
    );
    assert!(fill("{= nope =}", &[]).unwrap_err().contains("nope"));
}

#[test]
fn a_port_brings_the_files_beside_it_that_use_its_names_and_the_file_declaring_it() {
    // the command uses the Clock port; clock/in_memory.rs implements it, and the writer
    // must see it to extend the port without breaking it
    let d = git_project();
    write(
        d.path(),
        "src/ports/clock/mod.rs",
        "pub mod in_memory;\n\npub trait Clock {\n    fn now(&self) -> u32;\n}\n",
    );
    write(
        d.path(),
        "src/ports/clock/in_memory.rs",
        "use super::Clock;\n\npub struct FixedClock;\n\nimpl Clock for FixedClock {\n    fn now(&self) -> u32 { 1 }\n}\n",
    );
    write(d.path(), "src/ports/clock/notes.rs", "pub fn clocks() {}\n");
    // ports/mod.rs declares the clock folder: a step that adds a port sees where it goes
    write(d.path(), "src/ports/mod.rs", "pub mod clock;\n");
    write(d.path(), "src/ports/other.rs", "pub fn clockwork() {}\n");
    write(
        d.path(),
        "src/commands/seat.rs",
        "use crate::ports::clock::Clock;\n\nfn f() {}\n",
    );
    let found = related(d.path(), "rust", "src/commands/seat.rs");
    let names: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        names,
        [
            "src/ports/clock/mod.rs",
            "src/ports/clock/in_memory.rs",
            "src/ports/mod.rs"
        ]
    );
}

#[test]
fn a_python_protocol_brings_its_adapters_and_contract_test_but_not_unrelated_neighbours() {
    let d = git_project();
    let port = "src/bistro/ports/table_repository";
    write(
        d.path(),
        &format!("{port}/table_repository.py"),
        "class TableRepository:\n    def get(self, id): ...\n",
    );
    write(
        d.path(),
        &format!("{port}/in_memory_table_repository.py"),
        "class InMemoryTableRepository:\n    \"\"\"Keeps the TableRepository contract.\"\"\"\n",
    );
    write(
        d.path(),
        &format!("{port}/table_repository__test.py"),
        "from bistro.ports.table_repository.table_repository import TableRepository\n",
    );
    write(d.path(), &format!("{port}/readme.txt"), "TableRepository\n");
    write(d.path(), &format!("{port}/tables.py"), "TABLES = 3\n");
    write(
        d.path(),
        "src/bistro/commands/close.py",
        "from bistro.ports.table_repository.table_repository import TableRepository\n",
    );
    let found = related(d.path(), "python", "src/bistro/commands/close.py");
    let names: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        names,
        [
            "src/bistro/ports/table_repository/table_repository.py",
            "src/bistro/ports/table_repository/in_memory_table_repository.py",
            "src/bistro/ports/table_repository/table_repository__test.py",
        ]
    );
}

#[test]
fn files_beside_stop_at_four_for_one_import_and_skip_fenced_ones() {
    let d = git_project();
    write(d.path(), "src/a/port.py", "class Port:\n    pass\n");
    for i in 0..6 {
        write(d.path(), &format!("src/a/user{i}.py"), "x = Port\n");
    }
    write(d.path(), "src/main.py", "from a.port import Port\n");
    let (project, view) = project_with_vendored_grammars(d.path());
    let loaded = project.languages.load("python", &test_cache()).unwrap();
    let text = view.text(Path::new("src/main.py")).unwrap();
    let fenced = Path::new("src/a/user0.py");
    let found = related_files(&view, &loaded, Path::new("src/main.py"), &text, &|p| {
        p != fenced
    })
    .unwrap();
    let names: Vec<String> = found
        .files
        .iter()
        .map(|file| file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        names,
        [
            "src/a/port.py",
            "src/a/user1.py",
            "src/a/user2.py",
            "src/a/user3.py",
            "src/a/user4.py"
        ]
    );
    assert!(found.left_out.is_empty(), "{:?}", found.left_out);
}
