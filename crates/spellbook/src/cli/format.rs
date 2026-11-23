//! How the commands print rows.

/// Rows as columns, each column as wide as its widest cell, two spaces apart, each line
/// indented by `indent` and with no space at its end.
///
/// Example: `columns("  ", &[vec!["field", "a.x:10"], vec!["handle", "a.x:30"]])` is
/// `"  field   a.x:10\n  handle  a.x:30\n"`.
pub(super) fn columns(indent: &str, rows: &[Vec<String>]) -> String {
    let count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..count)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    rows.iter()
        .map(|row| {
            let padded: Vec<String> = row
                .iter()
                .zip(&widths)
                .map(|(cell, width)| format!("{cell:<width$}"))
                .collect();
            format!("{indent}{}\n", padded.join("  ").trim_end())
        })
        .collect()
}

/// `1 hole`, `3 holes`.
pub(super) fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
