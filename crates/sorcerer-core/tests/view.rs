#![allow(clippy::too_many_lines)]
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::view::CodebaseView;

fn view(d: &Path) -> FilesView {
    let mut p = Project::open(d).unwrap();
    p.languages = support::languages_with_vendored_grammars();
    FilesView::new(Arc::new(p), support::test_cache())
}

fn project() -> tempfile::TempDir {
    let d = support::git_project();
    support::write(d.path(), "src/a.py", "def f():\n    return 1\n");
    support::write(d.path(), ".gitignore", "build/\n");
    support::commit(d.path(), "a");
    d
}

#[test]
fn text_at_head_is_the_committed_text() {
    let d = project();
    support::write(d.path(), "src/a.py", "def f():\n    return 2\n");
    let v = view(d.path());
    assert_eq!(
        v.text_at(Path::new("src/a.py"), "HEAD").unwrap().as_deref(),
        Some("def f():\n    return 1\n")
    );
    assert_eq!(
        v.text(Path::new("src/a.py")).unwrap(),
        "def f():\n    return 2\n"
    );
}

#[test]
fn text_at_head_of_an_untracked_file_is_none() {
    let d = project();
    support::write(d.path(), "src/b.py", "x = 1\n");
    assert_eq!(
        view(d.path())
            .text_at(Path::new("src/b.py"), "HEAD")
            .unwrap(),
        None
    );
}

#[test]
fn text_at_a_bad_rev_is_an_error() {
    let d = project();
    assert!(
        view(d.path())
            .text_at(Path::new("src/a.py"), "no-such-branch")
            .is_err()
    );
}

#[test]
fn changed_since_head_lists_edited_and_untracked_files() {
    let d = project();
    support::write(d.path(), "src/a.py", "def f():\n    return 2\n");
    support::write(d.path(), "src/b.py", "x = 1\n");
    support::write(d.path(), "build/out.py", "x = 1\n");
    let got = view(d.path()).changed_since("HEAD").unwrap();
    assert_eq!(
        got,
        vec![PathBuf::from("src/a.py"), PathBuf::from("src/b.py")]
    );
}

#[test]
fn units_of_a_python_file_and_none_for_markdown() {
    let d = project();
    support::write(d.path(), "README.md", "# hi\n");
    let v = view(d.path());
    assert_eq!(v.units(Path::new("src/a.py")).unwrap()[0].name, "f");
    assert!(v.units(Path::new("README.md")).unwrap().is_empty());
}

#[test]
fn facts_of_a_unit() {
    let d = project();
    let v = view(d.path());
    let u = v.units(Path::new("src/a.py")).unwrap().remove(0);
    assert!(!v.facts(Path::new("src/a.py"), &u).unwrap().body_empty);
}

#[test]
fn a_non_utf8_file_is_not_text() {
    let d = project();
    std::fs::write(d.path().join("bin.dat"), [0xff, 0xfe, 0x00]).unwrap();
    let e = view(d.path()).text(Path::new("bin.dat")).unwrap_err();
    assert!(matches!(e, sorcerer_core::Error::NotText { .. }), "{e}");
}

#[test]
fn grep_finds_lines_and_respects_gitignore() {
    let d = project();
    support::write(d.path(), "build/out.py", "return 1\n");
    let hits = view(d.path()).grep("return 1", None).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        (hits[0].path.as_path(), hits[0].line, hits[0].text.as_str()),
        (Path::new("src/a.py"), 2, "    return 1")
    );
    assert!(
        view(d.path())
            .grep("return", Some("**/*.md"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn glob_lists_matching_files() {
    let d = project();
    support::write(d.path(), "src/c/d.py", "");
    assert_eq!(
        view(d.path()).glob("src/**/*.py").unwrap(),
        vec![PathBuf::from("src/a.py"), PathBuf::from("src/c/d.py")]
    );
}

#[test]
fn diff_shows_an_uncommitted_change() {
    let d = support::git_project();
    support::write(d.path(), "a.py", "x = 1\n");
    support::commit(d.path(), "a");
    support::write(d.path(), "a.py", "x = 2\n");
    let got = view(d.path()).diff(Path::new("a.py"), "HEAD").unwrap();
    assert!(got.contains("-x = 1"), "{got}");
    assert!(got.contains("+x = 2"), "{got}");
}

#[test]
fn diff_of_an_untracked_file_is_empty() {
    let d = project();
    support::write(d.path(), "src/b.py", "x = 1\n");
    assert_eq!(
        view(d.path()).diff(Path::new("src/b.py"), "HEAD").unwrap(),
        ""
    );
}

#[test]
fn merge_base_of_a_branch_is_the_branch_point() {
    let d = support::git_project();
    support::write(d.path(), "a.py", "x = 1\n");
    support::commit(d.path(), "first");
    let first = support::git(d.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    support::git(d.path(), &["checkout", "-q", "-b", "f"]);
    support::write(d.path(), "b.py", "y = 1\n");
    support::commit(d.path(), "on f");
    support::git(d.path(), &["checkout", "-q", "main"]);
    support::write(d.path(), "c.py", "z = 1\n");
    support::commit(d.path(), "on main");
    support::git(d.path(), &["checkout", "-q", "f"]);
    assert_eq!(view(d.path()).merge_base("main").unwrap(), first);
}

#[test]
fn merge_base_of_an_unknown_rev_names_it() {
    let d = project();
    let err = view(d.path()).merge_base("nosuch").unwrap_err();
    assert!(err.to_string().contains("nosuch"), "{err}");
}

#[test]
fn units_at_head_are_the_committed_units() {
    let d = project();
    support::write(
        d.path(),
        "src/a.py",
        "def f():\n    return 1\n\ndef g():\n    pass\n",
    );
    let v = view(d.path());
    let names = |us: Vec<sorcerer_core::domain::unit::Unit>| -> Vec<String> {
        us.into_iter().map(|u| u.name).collect()
    };
    assert_eq!(
        names(v.units_at(Path::new("src/a.py"), "HEAD").unwrap()),
        ["f"]
    );
    assert_eq!(names(v.units(Path::new("src/a.py")).unwrap()), ["f", "g"]);
    assert!(
        v.units_at(Path::new("src/new.py"), "HEAD")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn changed_since_lists_both_sides_of_a_rename() {
    let d = project();
    support::git(d.path(), &["mv", "src/a.py", "src/b.py"]);
    assert_eq!(
        view(d.path()).changed_since("HEAD").unwrap(),
        vec![PathBuf::from("src/a.py"), PathBuf::from("src/b.py")]
    );
}

#[test]
fn exists_says_whether_a_path_is_in_the_working_tree() {
    let d = project();
    std::fs::write(d.path().join("logo.png"), [0u8, 159, 146, 150]).unwrap();
    let v = view(d.path());
    assert!(v.exists(Path::new("src/a.py")));
    assert!(v.exists(Path::new("logo.png")));
    assert!(!v.exists(Path::new("src/gone.py")));
}
