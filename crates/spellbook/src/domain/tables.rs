//! Lookup tables: `tables.toml` beside the generators holds data a template looks up by key,
//! such as what each kind of field is in each language of a stack. Spellbook knows nothing of
//! the columns: they are the pack's.
//!
//! ```toml
//! [kinds.text]
//! rust   = "String"
//! sqlite = "TEXT"
//!
//! [kinds.int]
//! rust   = "i64"
//! sqlite = "INTEGER"
//! ```
//!
//! A template reads a cell as `{= kinds[f.kind].rust =}`.

use std::collections::BTreeMap;

use sorcerer_core::domain::template_blocks::Datum;
use toml::{Table, Value};

/// Every table, by name: each row by its key, each row's cells by column, every cell text.
///
/// Example: the file above is `{ kinds: { int: { rust: "i64", sqlite: "INTEGER" }, text: … } }`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tables(pub BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>);

/// The rows of one table: row key, then its cells by column.
pub type Rows = BTreeMap<String, BTreeMap<String, String>>;

/// The file's name, as errors and the generators directory name it.
pub const TABLES_FILE: &str = "tables.toml";

/// Reads `tables.toml`. Every top-level name is a table, each of its entries a row, and every
/// cell text; anything else is an error naming where it is.
///
/// Example: `[kinds.text]\nrust = 1` is refused with
/// "tables.toml [kinds.text] rust: a value is text".
pub fn read_tables(text: &str) -> Result<Tables, String> {
    let top: Table = toml::from_str(text).map_err(|e| format!("{TABLES_FILE}: {}", e.message()))?;
    top.iter()
        .map(|(name, value)| Ok((name.clone(), table_rows(name, value)?)))
        .collect::<Result<_, String>>()
        .map(Tables)
}

impl Tables {
    /// The rows of the table with this name, when there is one.
    pub fn rows(&self, table: &str) -> Option<&Rows> {
        self.0.get(table)
    }

    /// The table names, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// Each table as a template reads it: a record of rows, each a record of text cells.
    pub fn datum(&self) -> Vec<(String, Datum)> {
        self.0
            .iter()
            .map(|(name, rows)| (name.clone(), rows_datum(rows)))
            .collect()
    }
}

fn rows_datum(rows: &Rows) -> Datum {
    Datum::Record(
        rows.iter()
            .map(|(key, cells)| {
                let cells = cells
                    .iter()
                    .map(|(column, cell)| (column.clone(), Datum::Text(cell.clone())))
                    .collect();
                (key.clone(), Datum::Record(cells))
            })
            .collect(),
    )
}

fn table_rows(name: &str, value: &Value) -> Result<Rows, String> {
    let rows = value.as_table().ok_or_else(|| {
        format!("{TABLES_FILE}: `{name}` is a table of rows ([{name}.row] column = \"text\")")
    })?;
    rows.iter()
        .map(|(key, row)| Ok((key.clone(), row_cells(name, key, row)?)))
        .collect()
}

fn row_cells(name: &str, key: &str, row: &Value) -> Result<BTreeMap<String, String>, String> {
    let cells = row.as_table().ok_or_else(|| {
        format!("{TABLES_FILE} [{name}] {key}: a row is a table ([{name}.{key}] column = \"text\")")
    })?;
    cells
        .iter()
        .map(|(column, cell)| match cell {
            Value::String(text) => Ok((column.clone(), text.clone())),
            _ => Err(format!(
                "{TABLES_FILE} [{name}.{key}] {column}: a value is text"
            )),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_cells_are_read() {
        let tables =
            read_tables("[kinds.text]\nrust = \"String\"\n[kinds.int]\nrust = \"i64\"\n").unwrap();
        let rows = tables.rows("kinds").unwrap();
        assert_eq!(rows["int"]["rust"], "i64");
        assert_eq!(tables.names().collect::<Vec<_>>(), ["kinds"]);
    }

    #[test]
    fn a_value_that_is_not_text_is_refused() {
        assert_eq!(
            read_tables("[kinds.text]\nrust = 1\n").unwrap_err(),
            "tables.toml [kinds.text] rust: a value is text"
        );
        assert!(
            read_tables("kinds = 1\n")
                .unwrap_err()
                .contains("a table of rows")
        );
        assert!(
            read_tables("[kinds]\ntext = 1\n")
                .unwrap_err()
                .contains("a row is a table")
        );
    }
}
