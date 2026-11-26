//! A finding: one place a rule's convention does not hold.

use serde::{Deserialize, Serialize};
use sorcerer_core::domain::fnv::fnv64_hex;

use crate::domain::rule::Severity;

/// The rule id of the warning on an allow comment that gives no reason.
pub const ALLOW_WITHOUT_REASON: &str = "allow-without-reason";
/// The rule id of the warning on an allow comment that names no rule.
pub const UNKNOWN_RULE: &str = "unknown-rule";

/// One finding, as `report.json` keeps it and `check --json` prints it.
///
/// Example: `{ rule: "commands-use-ports", file: "src/bistro/booking/commands/a.txt", line: 4,
/// severity: Warn, message: "Commands talk to ports", fingerprint: "3fa9c01" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    /// Relative to the project root, with `/`.
    pub file: String,
    /// From 1.
    pub line: u32,
    pub severity: Severity,
    pub message: String,
    /// Stays the same when the code is moved or re-indented, so a later milestone can tell an
    /// old finding from a new one.
    pub fingerprint: String,
    /// The found node's text, for the fixes a finding offers; not kept in `report.json`.
    #[serde(skip)]
    pub node_text: String,
}

/// What a rule found in one file, before the allow comments are read: the rule, the line the
/// matched node starts on (from 1), the node's text and where the node is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFinding {
    pub rule: String,
    pub line: u32,
    pub node_text: String,
    /// The matched node's kind in the grammar (`import_statement`).
    pub kind: String,
    pub span: Span,
}

/// Where a node of a file is: its first and last rows (from 0, as the parser counts them) and its
/// bytes (the end not included).
///
/// Example: `import sqlite3` alone on the file's fourth line is rows 3 to 3; when the file's first
/// three lines take 40 bytes, its bytes are 40 to 54.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start_row: usize,
    pub end_row: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// Seven hex digits from the rule and the node's text with every run of whitespace as one
/// space.
///
/// Example: `import   sqlite3` and `import sqlite3` have one fingerprint under one rule.
pub fn fingerprint(rule: &str, node_text: &str) -> String {
    let collapsed: Vec<&str> = node_text.split_whitespace().collect();
    let key = format!("{rule}\n{}", collapsed.join(" "));
    fnv64_hex(key.as_bytes())[..7].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_ignores_whitespace() {
        assert_eq!(
            fingerprint("r", "import   sqlite3\n"),
            fingerprint("r", " import sqlite3")
        );
        assert_ne!(
            fingerprint("r", "import sqlite3"),
            fingerprint("s", "import sqlite3")
        );
        assert_eq!(fingerprint("r", "x").len(), 7);
    }
}
