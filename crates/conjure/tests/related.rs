//! Related files, the language tables and the path templates, on the vendored grammars.

mod support;

use std::path::Path;

use conjure::adapters::imports::related_files;
use conjure::domain::language_tables::{test_table, tests_path};
use conjure::domain::templates::fill;
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
        .map(|(p, t)| (p.to_string_lossy().replace('\\', "/"), t))
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
