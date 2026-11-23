//! A copy of bistro (conjure's Python corpus base) with a `command` generator under
//! `.sorcerer/spellbook/generators/command/`: olifant's python-htmx `command`, ported (its
//! `field` and `type` are placeholder holes, and `handle`'s body is a block hole).

use std::path::{Path, PathBuf};

use tempfile::TempDir;

pub const COMMAND_TOML: &str = r#"name        = "Command"
summary     = "A command of a service, with its test"
description = "commands/NAME.py (its Input, a failure in a one-per-line union, handle) and NAME__test.py, a failing stub wired up, ready for the first case"
language    = "python"
examples    = ["spellbook new command --set service=booking --set command=cancel_booking"]

[inputs.app]
type   = "enum"
one_of = "apps"
prompt = "The app (the folder under src/ with main.py)"

[inputs.service]
type   = "enum"
one_of = "services"
prompt = "The service it belongs to"

[inputs.command]
type   = "ident"
prompt = "The command, a verb phrase (cancel_booking, close_table)"

[derive]
Command  = "{= command|pascal =}"
command_ = "{= command|snake =}"
dir      = "src/{= app =}/{= service|snake =}/commands"
module   = "{= app =}.{= service|snake =}.commands.{= command|snake =}"

[query.apps]
glob  = "src/*/main.py"
value = "parent folder"

[query.services]
glob  = "src/{= app =}/*/models"
value = "parent folder"

[[edits]]
create    = "{= dir =}/__init__.py"
text      = ""
on_exists = "skip"

[[edits]]
create   = "{= dir =}/{= command_ =}.py"
template = "command.py.dt"

[[edits]]
create   = "{= dir =}/{= command_ =}__test.py"
template = "command__test.py.dt"

# The docstring starts as the name in words.
[holes.purpose]
source = "derived"
value  = "{= command|sentence =}."

[holes.value]
source = "derived"
value  = "None"

[holes.failure]
source = "derived"
value  = "NotFound"
"#;

pub const COMMAND_PY: &str = r#""""{= ? purpose: text | what this command does =}"""

from dataclasses import dataclass

from {= app =}.shared.result import Ok


@dataclass(frozen=True)
class {= Command =}Input:
    {= ? field: ident | its first input =}: {= ? type: type | that input's type =}


@dataclass(frozen=True)
class {= ? failure: ident | the first way it can fail =}:
    reason: str


type {= Command =}Failure = (
    {= ? failure: ident | the first way it can fail =}
)

type {= Command =}Output = Ok[{= ? value: ident | what it returns when it succeeds =}] | {= Command =}Failure


class {= Command =}:
    def __init__(self) -> None:
        """Takes the ports it uses; main.py passes their adapters."""

    def handle(self, input: {= Command =}Input) -> {= Command =}Output:
        {= ? handle: block | its first case, from the test file beside it =}
"#;

pub const COMMAND_TEST_PY: &str = r#"from {= module =} import {= Command =}, {= Command =}Input
from {= app =}.shared.result import Ok


def command() -> {= Command =}:
    return {= Command =}()


def test_{= command_ =}_succeeds() -> None:
    {= command_ =} = command()
    result = {= command_ =}.handle({= Command =}Input({= ? field: ident | its first input =}="1"))
    assert result == Ok(None)
"#;

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
