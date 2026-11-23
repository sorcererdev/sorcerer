//! A copy of bistro (conjure's Python corpus base) with a `command` generator under
//! `.sorcerer/spellbook/generators/command/`: olifant's python-htmx `command`, ported (its
//! `field` and `type` are placeholder holes, and `handle`'s body is a block hole).

use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// The corpus's `command` generator: one source of truth for the tests and the corpus.
pub const COMMAND_TOML: &str =
    include_str!("../../../../corpus/spellbook/bistro/spellbook/generators/command/generator.toml");
pub const COMMAND_PY: &str = include_str!(
    "../../../../corpus/spellbook/bistro/spellbook/generators/command/templates/command.py.dt"
);
pub const COMMAND_TEST_PY: &str = include_str!(
    "../../../../corpus/spellbook/bistro/spellbook/generators/command/templates/command__test.py.dt"
);

/// conjure's bistro base, the Python corpus project.
pub fn bistro_base() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/conjure/bistro/base")
}

/// A fresh copy of bistro with the `command` generator.
pub fn bistro_project() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&bistro_base(), dir.path());
    std::fs::create_dir_all(dir.path().join(".sorcerer")).unwrap();
    write_generator(
        dir.path(),
        "command",
        COMMAND_TOML,
        &[
            ("command.py.dt", COMMAND_PY),
            ("command__test.py.dt", COMMAND_TEST_PY),
        ],
    );
    dir
}

/// Writes a generator's `generator.toml` and templates into a project.
pub fn write_generator(root: &Path, id: &str, toml: &str, templates: &[(&str, &str)]) {
    let dir = root.join(".sorcerer/spellbook/generators").join(id);
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    std::fs::write(dir.join("generator.toml"), toml).unwrap();
    for (name, text) in templates {
        std::fs::write(dir.join("templates").join(name), text).unwrap();
    }
}

/// Writes a file into a project, its folders created.
pub fn write_file(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Copies a tree into an existing directory, leaving out virtual environments and caches.
pub fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if [".venv", "__pycache__", ".pytest_cache"].contains(&name.to_string_lossy().as_ref()) {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
