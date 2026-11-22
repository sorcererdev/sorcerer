//! Words and lines the reports share: counts, dollars, and one record as `conjure log` prints it.

use chrono::FixedOffset;

use crate::domain::record::{ChangeRecord, Choice};
use sorcerer_core::domain::paths::forward_slashes;

/// Dollars to the tenth of a cent: `$0.042`.
pub fn usd(dollars: f64) -> String {
    format!("${:.3}", if dollars == 0.0 { 0.0 } else { dollars })
}

/// A count and its noun, plural unless it is 1: `1 change`, `2 changes`, `0 changes`.
pub fn count(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// What became of a change, in the word the record keeps.
pub fn choice_word(choice: Choice) -> &'static str {
    match choice {
        Choice::Kept => "kept",
        Choice::Undone => "undone",
        Choice::Replaced => "replaced",
        Choice::Dropped => "dropped",
        Choice::NoChange => "no-change",
        Choice::Failed => "failed",
        Choice::Unanswered => "unanswered",
    }
}

/// One record as a line of `conjure log`, in the user's time zone:
/// `2026-11-22 12:00 · src/cart.py:12 · Cart.total · fill total · haiku · $0.001 · kept`.
pub fn record_line(record: &ChangeRecord, offset: FixedOffset) -> String {
    let when = record.at.with_timezone(&offset).format("%Y-%m-%d %H:%M");
    let goal = record.goal.as_deref().unwrap_or("(no goal)");
    let cost = usd(record.cost_usd.unwrap_or(0.0));
    format!(
        "{when} · {}:{} · {} · {goal} · {} · {cost} · {}",
        forward_slashes(std::path::Path::new(&record.file)),
        record.line,
        record.unit,
        record.model,
        choice_word(record.choice)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_dollars() {
        assert_eq!(count(1, "change"), "1 change");
        assert_eq!(count(0, "line"), "0 lines");
        assert_eq!(usd(0.0), "$0.000");
        assert_eq!(usd(-0.0), "$0.000");
        assert_eq!(usd(0.0415), "$0.042");
    }
}
