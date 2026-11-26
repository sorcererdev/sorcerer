//! Anchor phrases over Python files shaped like bistro's commands and their tests.

mod support;

use std::path::Path;

use spellbook::adapters::query_anchors::QueryAnchors;
use spellbook::domain::anchor::{Anchor, Place, Resolution};
use spellbook::ports::anchors::Anchors;
use support::*;

const CLOSE_TABLE: &str = r#""""Close a table for the night."""

from dataclasses import dataclass
import sqlite3

from bistro.shared.result import Ok


class CloseTable:
    def __init__(self) -> None:
        """Takes the ports it uses; main.py passes their adapters."""

    def handle(self, input: str) -> None:
        conn = sqlite3.connect(input)
        return conn
"#;

const CLOSE_TABLE_TEST: &str = r#"from bistro.booking.commands.close_table import CloseTable
from bistro.shared.result import Ok


def command() -> CloseTable:
    return CloseTable()


def query() -> CloseTable:
    return CloseTable(
        bookings=InMemoryBookingRepository(),
        tables=tables,
    )
"#;

const ONE_PER_LINE: &str = r#"class CreateBooking:
    def __init__(
        self,
        bookings: BookingRepository,
        tables: TableRepository
    ) -> None:
        self.bookings = bookings
"#;

fn resolve(text: &str, phrase: &str) -> Resolution {
    let dir = bistro_project();
    let wired = wired_at(dir.path());
    let anchors = QueryAnchors::new(&wired.grammars);
    anchors
        .resolve(
            Path::new("src/bistro/booking/commands/close_table.py"),
            text,
            &Anchor::parse(phrase).unwrap(),
        )
        .unwrap()
}

fn lines(row: usize, indent: &str, suffix: &str) -> Resolution {
    Resolution::At(Place::Lines {
        row,
        indent: indent.into(),
        suffix: suffix.into(),
    })
}

fn in_line(row: usize, column: usize, prefix: &str) -> Resolution {
    Resolution::At(Place::InLine {
        row,
        column,
        prefix: prefix.into(),
        suffix: String::new(),
    })
}

#[test]
fn end_of_imports_is_after_the_last_top_level_import() {
    assert_eq!(resolve(CLOSE_TABLE, "end of imports"), lines(6, "", ""));
    assert_eq!(resolve(CLOSE_TABLE, "start of imports"), lines(2, "", ""));
    assert_eq!(
        resolve(CLOSE_TABLE_TEST, "end of imports"),
        lines(2, "", "")
    );
    assert_eq!(
        resolve("\"\"\"A module.\"\"\"\n\nX = 1\n", "end of imports"),
        lines(1, "", "")
    );
}

#[test]
fn a_method_body_ends_after_its_last_statement() {
    assert_eq!(
        resolve(CLOSE_TABLE, "end of function CloseTable.__init__"),
        lines(11, "        ", "")
    );
    assert_eq!(
        resolve(CLOSE_TABLE, "end of function handle"),
        lines(15, "        ", "")
    );
    assert_eq!(
        resolve(CLOSE_TABLE, "end of class CloseTable"),
        lines(15, "    ", "")
    );
    assert_eq!(
        resolve(CLOSE_TABLE, "end of function Nope.__init__"),
        Resolution::Missing {
            what: "function `Nope.__init__`".into()
        }
    );
}

#[test]
fn a_one_line_parameter_list_takes_the_new_one_on_its_line() {
    // `    def __init__(self) -> None:`: `self` ends at column 21.
    assert_eq!(
        resolve(CLOSE_TABLE, "end of parameters in CloseTable.__init__"),
        in_line(9, 21, ", ")
    );
}

#[test]
fn a_one_per_line_parameter_list_takes_a_line_of_its_own() {
    // No separator after the last item: the separator and a new line join its line.
    assert_eq!(
        resolve(ONE_PER_LINE, "end of parameters in CreateBooking.__init__"),
        in_line(4, 31, ",\n        ")
    );
    let with_comma = ONE_PER_LINE.replace("TableRepository\n", "TableRepository,\n");
    assert_eq!(
        resolve(&with_comma, "end of parameters in CreateBooking.__init__"),
        lines(5, "        ", ",")
    );
}

#[test]
fn calls_resolve_by_name_and_narrow_with_in() {
    assert_eq!(
        resolve(CLOSE_TABLE_TEST, "end of call CloseTable in command"),
        in_line(5, 22, "")
    );
    assert_eq!(
        resolve(CLOSE_TABLE_TEST, "end of call CloseTable in query"),
        lines(12, "        ", ",")
    );
    assert_eq!(
        resolve(CLOSE_TABLE_TEST, "end of call CloseTable"),
        Resolution::Several {
            what: "call `CloseTable`".into(),
            count: 2
        }
    );
}
