//! How the commands print tables.

/// A table: each column as wide as its widest cell, columns two spaces apart, no space at the
/// end of a line.
///
/// Example:
/// ```text
/// day         changes
/// 2026-11-22  2
/// ```
pub(super) fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..header.len())
        .map(|column| {
            let cells = rows.iter().filter_map(|row| row.get(column));
            let widest_cell = cells.map(|cell| cell.chars().count()).max().unwrap_or(0);
            header[column].chars().count().max(widest_cell)
        })
        .collect();
    let line = |cells: Vec<&str>| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        format!("{}\n", padded.join("  ").trim_end())
    };
    let header_line = line(header.to_vec());
    let row_lines = rows
        .iter()
        .map(|row| line(row.iter().map(String::as_str).collect()));
    std::iter::once(header_line).chain(row_lines).collect()
}
