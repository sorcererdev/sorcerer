//! Anchor phrases over Python and Rust files shaped like bistro's commands and their tests.
//! Every phrase resolves through the language's query captures, so the same words work in both.

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
    resolve_in("src/bistro/booking/commands/close_table.py", text, phrase)
}

fn resolve_rust(text: &str, phrase: &str) -> Resolution {
    resolve_in("src/booking/commands/cancel_booking.rs", text, phrase)
}

fn resolve_in(file: &str, text: &str, phrase: &str) -> Resolution {
    let dir = bistro_project();
    let wired = wired_at(dir.path());
    let anchors = QueryAnchors::new(&wired.grammars);
    anchors
        .resolve(Path::new(file), text, &Anchor::parse(phrase).unwrap())
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
    in_line_with(row, column, prefix, "")
}

fn in_line_with(row: usize, column: usize, prefix: &str, suffix: &str) -> Resolution {
    Resolution::At(Place::InLine {
        row,
        column,
        prefix: prefix.into(),
        suffix: suffix.into(),
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

#[test]
fn a_comment_in_a_body_or_a_list_is_not_an_item() {
    let text = "class A:\n    def f(self, a):\n        x = 1\n        # the end\n";
    assert_eq!(
        resolve(text, "end of function A.f"),
        lines(3, "        ", "")
    );
    let one = "class A:\n    def f(\n        self,  # me\n    ) -> None:\n        pass\n";
    assert_eq!(
        resolve(one, "end of parameters in A.f"),
        lines(3, "        ", ",")
    );
}

#[test]
fn a_list_of_one_takes_the_languages_separator() {
    // `[syntax] list_separator` in language.toml: nothing between items to read it from.
    assert_eq!(
        resolve(CLOSE_TABLE, "start of parameters in CloseTable.__init__"),
        in_line_with(9, 17, "", ", ")
    );
    assert_eq!(
        resolve(
            "def command():\n    return CloseTable()\n",
            "end of call CloseTable in command"
        ),
        in_line(1, 22, "")
    );
}

#[test]
fn a_module_docstring_and_leading_comments_come_before_the_first_import() {
    let text = "# a header\n\"\"\"The module.\"\"\"\n\nX = 1\n";
    assert_eq!(resolve(text, "end of imports"), lines(2, "", ""));
    let rust = "//! The module.\n//! More.\n\npub struct A;\n";
    assert_eq!(resolve_rust(rust, "end of imports"), lines(2, "", ""));
}

const CANCEL_BOOKING: &str = r#"//! Cancel a booking.

use std::rc::Rc;

use crate::booking::ports::id_generator::IdGenerator;

pub struct CancelBooking {
    ids: Rc<dyn IdGenerator>,
}

impl CancelBooking {
    pub fn new<T: Into<String>>(ids: Rc<dyn IdGenerator>) -> CancelBooking {
        CancelBooking { ids }
    }

    pub fn handle(&self, id: &str) -> Result<(), String> {
        // nothing yet
        todo!("hole handle: cancel it")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::booking::ports::id_generator::in_memory::SequentialIds;

    fn command() -> CancelBooking {
        CancelBooking::new(Rc::new(SequentialIds::default()))
    }
}
"#;

#[test]
fn rust_imports_end_after_the_last_top_level_use() {
    // the `use` lines inside `mod tests` are the module's, not the file's
    assert_eq!(
        resolve_rust(CANCEL_BOOKING, "end of imports"),
        lines(5, "", "")
    );
    assert_eq!(
        resolve_rust(CANCEL_BOOKING, "start of imports"),
        lines(2, "", "")
    );
}

#[test]
fn rust_bodies_are_the_textobjects_inner_items() {
    assert_eq!(
        resolve_rust(CANCEL_BOOKING, "end of class CancelBooking"),
        lines(8, "    ", "")
    );
    assert_eq!(
        resolve_rust(CANCEL_BOOKING, "end of function CancelBooking::handle"),
        lines(18, "        ", "")
    );
    assert_eq!(
        resolve_rust(CANCEL_BOOKING, "start of function CancelBooking::handle"),
        lines(17, "        ", "")
    );
}

#[test]
fn rust_parameters_are_the_functions_own_list_not_its_generics() {
    // `ids: Rc<dyn IdGenerator>` ends at column 56 of row 11, after `<T: Into<String>>`
    assert_eq!(
        resolve_rust(CANCEL_BOOKING, "end of parameters in CancelBooking::new"),
        in_line(11, 56, ", ")
    );
}

#[test]
fn rust_calls_include_a_path_call_and_a_struct_literal() {
    // `CancelBooking::new(Rc::new(SequentialIds::default()))`: after the last argument
    assert_eq!(
        resolve_rust(CANCEL_BOOKING, "end of call CancelBooking::new in command"),
        in_line(27, 60, ", ")
    );
    // the struct literal `CancelBooking { ids }` builds it: after `ids`
    assert_eq!(
        resolve_rust(
            CANCEL_BOOKING,
            "end of call CancelBooking in CancelBooking::new"
        ),
        in_line(12, 27, ", ")
    );
}
