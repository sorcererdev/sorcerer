//! `conjure log`: the last changes the editor recorded.

use std::io::Write;

use anyhow::Result;
use chrono::FixedOffset;

use crate::domain::log_text::record_line;
use crate::ports::change_store::ChangeStore;
use crate::usecases::show_log::last_changes;

/// How many changes `conjure log` shows when not asked for a number.
pub(super) const DEFAULT_COUNT: usize = 10;

/// Prints the last `n` records, the newest first, one line each. With none recorded yet it
/// says so on stderr and still exits 0: an empty log is not an error.
pub(super) fn show_log(
    store: &dyn ChangeStore,
    n: usize,
    offset: FixedOffset,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let records = last_changes(store, n)?;
    if records.is_empty() {
        writeln!(
            err,
            "conjure: no changes recorded yet (the editor records each change it conjures)"
        )?;
        return Ok(0);
    }
    for record in &records {
        writeln!(out, "{}", record_line(record, offset))?;
    }
    Ok(0)
}
