//! Two regions in one file, set up one after the other in either order, with `init_at`
//! phrases that resolve next to each other: neither block lands inside the other, and every
//! hand-written line is either still there or adopted into a block. On a Rust `mod.rs` and a
//! Python `__init__.py`.

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

const RUST_REGIONS: &str = r#"
[regions.part-files]
file    = "src/booking/parts/mod.rs"
query   = { glob = "{= dir =}/*.rs", exclude = ["{= dir =}/mod.rs"] }
item    = "pub mod {= stem =};"
init_at = ["after last line matching ^pub mod ", "end of imports"]

[regions.part-folders]
file    = "src/booking/parts/mod.rs"
query   = { glob = "{= dir =}/*/mod.rs" }
item    = "pub mod {= parent =};"
init_at = ["after last line matching ^pub mod ", "end of imports"]
"#;

/// The hand-written list: two files and one folder declared, mixed; one folder not yet.
const RUST_MOD: &str = "//! The parts of a booking.\n\
\n\
pub mod alpha;\n\
pub mod gamma;\n\
pub mod beta;\n\
\n\
pub fn helper() {}\n";

/// bistro-rs with only the two `parts` regions, and `src/booking/parts/` holding two files
/// (`alpha.rs`, `beta.rs`) and two folders (`gamma/`, `delta/`).
fn rust_project() -> tempfile::TempDir {
    let dir = bistro_rs_project();
    let root = dir.path();
    write_file(root, ".sorcerer/spellbook/regions.toml", RUST_REGIONS);
    write_file(root, "src/booking/parts/mod.rs", RUST_MOD);
    write_file(root, "src/booking/parts/alpha.rs", "pub fn alpha() {}\n");
    write_file(root, "src/booking/parts/beta.rs", "pub fn beta() {}\n");
    write_file(
        root,
        "src/booking/parts/gamma/mod.rs",
        "pub fn gamma() {}\n",
    );
    write_file(
        root,
        "src/booking/parts/delta/mod.rs",
        "pub fn delta() {}\n",
    );
    dir
}

const PYTHON_REGIONS: &str = r#"
[regions.part-modules]
file    = "src/bistro/parts/__init__.py"
query   = { glob = "{= dir =}/*.py", exclude = ["{= dir =}/__init__.py"] }
item    = "from .{= stem =} import {= stem|pascal =}"
init_at = "end of imports"

[regions.part-packages]
file    = "src/bistro/parts/__init__.py"
query   = { glob = "{= dir =}/*/__init__.py" }
item    = "from . import {= parent =}"
init_at = "end of imports"
"#;

const PYTHON_INIT: &str = "\"\"\"The parts of a booking.\"\"\"\n\
\n\
from .alpha import Alpha\n\
from . import gamma\n\
from .beta import Beta\n\
\n\
__all__ = [\"Alpha\", \"Beta\"]\n";

/// bistro with only the two `parts` regions, and `src/bistro/parts/` holding two modules and
/// two packages.
fn python_project() -> tempfile::TempDir {
    let dir = bistro_project();
    let root = dir.path();
    write_file(root, ".sorcerer/spellbook/regions.toml", PYTHON_REGIONS);
    write_file(root, "src/bistro/parts/__init__.py", PYTHON_INIT);
    write_file(
        root,
        "src/bistro/parts/alpha.py",
        "class Alpha:\n    pass\n",
    );
    write_file(root, "src/bistro/parts/beta.py", "class Beta:\n    pass\n");
    write_file(root, "src/bistro/parts/gamma/__init__.py", "");
    write_file(root, "src/bistro/parts/delta/__init__.py", "");
    dir
}

/// Sets up the two regions in the order given, then checks the file: each block holds its own
/// items, neither block is inside the other, the lines outside the blocks are the hand-written
/// ones less those adopted, and a second sync is up to date.
fn both_set_up(
    dir: &Path,
    file: &str,
    order: [&str; 2],
    expected_blocks: &[(&str, &[&str])],
    kept_outside: &[&str],
) {
    for name in order {
        let (code, out, err) = run(dir, &["sync", "--init", name]);
        assert_eq!(code, 0, "--init {name}: {out}{err}");
    }
    let text = std::fs::read_to_string(dir.join(file)).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut outside: Vec<&str> = lines.clone();
    let mut spans = Vec::new();
    for (name, items) in expected_blocks {
        let open = lines
            .iter()
            .position(|line| line.contains(&format!("spellbook:region {name} ")))
            .unwrap_or_else(|| panic!("no opening marker for {name}:\n{text}"));
        let close = lines
            .iter()
            .position(|line| line.trim_end().ends_with(&format!("spellbook:end {name}")))
            .unwrap_or_else(|| panic!("no closing marker for {name}:\n{text}"));
        assert_eq!(
            &lines[open + 1..close],
            *items,
            "region {name} holds the wrong lines:\n{text}"
        );
        spans.push((open, close));
    }
    let (first, second) = (spans[0], spans[1]);
    assert!(
        first.1 < second.0 || second.1 < first.0,
        "one block is inside the other:\n{text}"
    );
    for (open, close) in spans {
        for line in &lines[open..=close] {
            if let Some(at) = outside.iter().position(|kept| kept == line) {
                outside.remove(at);
            }
        }
    }
    assert_eq!(outside, kept_outside, "lines outside the blocks:\n{text}");
    let (code, out, err) = run(dir, &["sync", "--check"]);
    assert_eq!(code, 0, "{out}{err}\n{text}");
}

const RUST_BLOCKS: &[(&str, &[&str])] = &[
    ("part-files", &["pub mod alpha;", "pub mod beta;"]),
    ("part-folders", &["pub mod delta;", "pub mod gamma;"]),
];
const RUST_OUTSIDE: &[&str] = &["//! The parts of a booking.", "", "", "pub fn helper() {}"];

#[test]
fn rust_files_then_folders() {
    let dir = rust_project();
    both_set_up(
        dir.path(),
        "src/booking/parts/mod.rs",
        ["part-files", "part-folders"],
        RUST_BLOCKS,
        RUST_OUTSIDE,
    );
}

#[test]
fn rust_folders_then_files() {
    let dir = rust_project();
    both_set_up(
        dir.path(),
        "src/booking/parts/mod.rs",
        ["part-folders", "part-files"],
        RUST_BLOCKS,
        RUST_OUTSIDE,
    );
}

const PYTHON_BLOCKS: &[(&str, &[&str])] = &[
    (
        "part-modules",
        &["from .alpha import Alpha", "from .beta import Beta"],
    ),
    (
        "part-packages",
        &["from . import delta", "from . import gamma"],
    ),
];
const PYTHON_OUTSIDE: &[&str] = &[
    "\"\"\"The parts of a booking.\"\"\"",
    "",
    "",
    "__all__ = [\"Alpha\", \"Beta\"]",
];

#[test]
fn python_modules_then_packages() {
    let dir = python_project();
    both_set_up(
        dir.path(),
        "src/bistro/parts/__init__.py",
        ["part-modules", "part-packages"],
        PYTHON_BLOCKS,
        PYTHON_OUTSIDE,
    );
}

#[test]
fn python_packages_then_modules() {
    let dir = python_project();
    both_set_up(
        dir.path(),
        "src/bistro/parts/__init__.py",
        ["part-packages", "part-modules"],
        PYTHON_BLOCKS,
        PYTHON_OUTSIDE,
    );
}

/// What the old `--init` left: the folders' block inside the files' block. A sync would
/// rewrite the outer block over the inner one, so it stops instead, naming the line.
#[test]
fn a_block_inside_another_stops_the_sync_naming_the_line() {
    let dir = rust_project();
    let nested = "//! The parts of a booking.\n\
\n\
// spellbook:region part-files (from src/booking/parts/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)\n\
pub mod alpha;\n\
pub mod beta;\n\
// spellbook:region part-folders (from src/booking/parts/*/mod.rs; edit .sorcerer/spellbook/regions.toml, not this block)\n\
pub mod delta;\n\
pub mod gamma;\n\
// spellbook:end part-folders\n\
// spellbook:end part-files\n\
\n\
pub fn helper() {}\n";
    write_file(dir.path(), "src/booking/parts/mod.rs", nested);
    let (code, out, err) = run(dir.path(), &["sync"]);
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(
        err,
        "spellbook: src/booking/parts/mod.rs:6: syncing region part-files would lose this line (`// spellbook:region part-folders (from src/booking/parts/*/mod.rs; edit .sorcerer/spellbook/regions.toml, not this block)`), which no block adopts; nothing was written\n"
    );
    let unchanged = std::fs::read_to_string(dir.path().join("src/booking/parts/mod.rs")).unwrap();
    assert_eq!(unchanged, nested);
}
