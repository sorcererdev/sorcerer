//! `EditorView`: the editor's unsaved text laid over the files on disk.
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use conjure::adapters::editor_view::EditorView;
use sorcerer_core::ports::view::CodebaseView;
use support::{commit, git_project, project_with_vendored_grammars, write};

const ON_DISK: &str = "def total(cart):\n    return 0\n";
const IN_THE_EDITOR: &str =
    "import os\n\n\ndef total(cart):\n    pass\n\n\ndef count(cart):\n    return 1\n";

fn view_with(dir: &Path, open: &[(&str, &str)]) -> EditorView {
    let (_, files) = project_with_vendored_grammars(dir);
    let open: BTreeMap<PathBuf, String> = open
        .iter()
        .map(|(path, text)| (PathBuf::from(path), (*text).to_string()))
        .collect();
    EditorView::new(files, open)
}

fn cart_on_disk() -> tempfile::TempDir {
    let dir = git_project();
    write(dir.path(), "src/cart.py", ON_DISK);
    commit(dir.path(), "cart");
    dir
}

#[test]
fn an_open_file_is_read_from_the_editor() {
    let dir = cart_on_disk();
    write(dir.path(), "src/other.py", "x = 1\n");
    let view = view_with(dir.path(), &[("src/cart.py", IN_THE_EDITOR)]);
    assert_eq!(view.text(Path::new("src/cart.py")).unwrap(), IN_THE_EDITOR);
    // a file the editor does not have open is the disk's
    assert_eq!(view.text(Path::new("src/other.py")).unwrap(), "x = 1\n");
}

#[test]
fn units_and_facts_follow_the_unsaved_text() {
    let dir = cart_on_disk();
    let view = view_with(dir.path(), &[("src/cart.py", IN_THE_EDITOR)]);
    let units = view.units(Path::new("src/cart.py")).unwrap();
    let names: Vec<(&str, u32)> = units
        .iter()
        .map(|unit| (unit.name.as_str(), unit.span.start))
        .collect();
    assert_eq!(names, [("total", 4), ("count", 8)]);
    let facts = view.facts(Path::new("src/cart.py"), &units[0]).unwrap();
    assert!(facts.body_empty, "{facts:?}");
}

#[test]
fn a_buffer_not_on_disk_exists() {
    let dir = cart_on_disk();
    let view = view_with(dir.path(), &[("src/new.py", "def fresh():\n    pass\n")]);
    assert!(view.exists(Path::new("src/new.py")));
    assert_eq!(
        view.text(Path::new("src/new.py")).unwrap(),
        "def fresh():\n    pass\n"
    );
    assert!(!view.exists(Path::new("src/absent.py")));
    assert!(view.exists(Path::new("src/cart.py")));
}

#[test]
fn history_comes_from_the_disk() {
    let dir = cart_on_disk();
    let view = view_with(dir.path(), &[("src/cart.py", IN_THE_EDITOR)]);
    let at_head = view.text_at(Path::new("src/cart.py"), "HEAD").unwrap();
    assert_eq!(at_head.as_deref(), Some(ON_DISK));
    let units_at_head = view.units_at(Path::new("src/cart.py"), "HEAD").unwrap();
    assert_eq!(units_at_head.len(), 1);
    // the editor's text is not on disk, so git sees no change and grep finds the disk's
    assert!(view.changed_since("HEAD").unwrap().is_empty());
    assert!(
        view.diff(Path::new("src/cart.py"), "HEAD")
            .unwrap()
            .is_empty()
    );
    let hits = view.grep("def count", None).unwrap();
    assert!(hits.is_empty(), "{hits:?}");
    assert_eq!(
        view.glob("src/*.py").unwrap(),
        [PathBuf::from("src/cart.py")]
    );
}

#[test]
fn a_file_answers_as_first_read_while_the_view_lives() {
    let dir = cart_on_disk();
    let view = view_with(dir.path(), &[]);
    assert_eq!(view.text(Path::new("src/cart.py")).unwrap(), ON_DISK);
    assert!(!view.exists(Path::new("src/new.py")));
    // the disk moves on: the view, a picture of one moment, does not
    write(
        dir.path(),
        "src/cart.py",
        "def total(cart):\n    return 1\n",
    );
    write(dir.path(), "src/new.py", "x = 1\n");
    assert_eq!(view.text(Path::new("src/cart.py")).unwrap(), ON_DISK);
    assert!(!view.exists(Path::new("src/new.py")));
    let units = view.units(Path::new("src/cart.py")).unwrap();
    let facts = view.facts(Path::new("src/cart.py"), &units[0]).unwrap();
    assert!(!facts.body_empty);
}
