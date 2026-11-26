//! bistro with the `use-port` and `add-port` fixes and a `close_table` command that opens
//! `sqlite3` itself: the finding a fix answers.

use std::path::Path;

use tempfile::TempDir;

use super::bistro_fixtures::{bistro_project, copy_tree, write_file};
use super::project_fixtures::use_corpus_rules;

pub const CLOSE_TABLE: &str = r#""""Close a table for the night."""

from dataclasses import dataclass
import sqlite3

from bistro.shared.result import Ok


@dataclass(frozen=True)
class CloseTableInput:
    table_id: str


class CloseTable:
    def __init__(self) -> None:
        """Takes the ports it uses; main.py passes their adapters."""

    def handle(self, input: CloseTableInput) -> Ok[None]:
        conn = sqlite3.connect("bistro.db")
        return Ok(None)
"#;

pub const CLOSE_TABLE_TEST: &str = r#"from bistro.booking.commands.close_table import CloseTable, CloseTableInput
from bistro.shared.result import Ok


def command() -> CloseTable:
    return CloseTable()


def test_close_table_succeeds() -> None:
    close_table = command()
    assert close_table.handle(CloseTableInput(table_id="1")) == Ok(None)
"#;

pub const CLOSE_TABLE_PATH: &str = "src/bistro/booking/commands/close_table.py";
pub const CLOSE_TABLE_TEST_PATH: &str = "src/bistro/booking/commands/close_table__test.py";

pub fn bistro_with_the_fix() -> TempDir {
    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    let generator_dir = dir.path().join(".sorcerer/spellbook/generators/use-port");
    std::fs::create_dir_all(&generator_dir).unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/spellbook/bistro/spellbook/generators/use-port"),
        &generator_dir,
    );
    write_file(dir.path(), CLOSE_TABLE_PATH, CLOSE_TABLE);
    write_file(dir.path(), CLOSE_TABLE_TEST_PATH, CLOSE_TABLE_TEST);
    dir
}
