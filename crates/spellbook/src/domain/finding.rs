//! A finding: one place a rule's convention does not hold.

use serde::{Deserialize, Serialize};
use sorcerer_core::domain::fnv::fnv64_hex;

use crate::domain::rule::Severity;

/// The rule id of the warning on an allow comment that gives no reason.
pub const ALLOW_WITHOUT_REASON: &str = "allow-without-reason";
/// The rule id of the warning on an allow comment that names no rule.
pub const UNKNOWN_RULE: &str = "unknown-rule";
/// The rule id of a region whose block is not what its files make it.
pub const REGION_STALE: &str = "region-stale";
/// The rule id of a file a region could keep that has no markers for it.
pub const REGION_NOT_SET_UP: &str = "region-not-set-up";
/// The rule id of a region whose markers are not a pair.
pub const REGION_BROKEN: &str = "region-broken";

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
    /// Stays the same when the code is moved, re-indented or edited elsewhere in the file, so
    /// the baseline can tell an old finding from a new one (see [`rule_finding_fingerprint`]).
    pub fingerprint: String,
    /// The found node's text, for the fixes a finding offers; not kept in `report.json`.
    #[serde(skip)]
    pub node_text: String,
    /// For a region's finding: the region and how many lines a sync would add and remove.
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionDrift>,
    /// Whether the baseline records this finding (`spellbook check --baseline`): known debt that
    /// never fails `check`. Left out of the JSON when it is not.
    #[serde(default, skip_serializing_if = "is_new")]
    pub existing: bool,
}

fn is_new(existing: &bool) -> bool {
    !*existing
}

/// A region's finding: which region, and what `spellbook sync` would do to its block.
///
/// Example: a deleted query file leaves `{ region: "modules", added: 0, removed: 1 }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionDrift {
    pub region: String,
    pub added: u32,
    pub removed: u32,
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
    /// The name of the innermost definition holding the node (`SeatWalkIn::handle`), `None`
    /// at the top level of the file.
    pub unit: Option<String>,
}

impl RawFinding {
    /// This finding's fingerprint in the file's `text`; see [`rule_finding_fingerprint`].
    pub fn fingerprint(&self, text: &str) -> String {
        let line = text
            .lines()
            .nth(self.line.saturating_sub(1) as usize)
            .unwrap_or_default();
        rule_finding_fingerprint(&self.rule, self.unit.as_deref(), line)
    }
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

/// Seven hex digits naming a rule's finding by what it is, not by where it sits: the rule, the
/// name of the innermost definition holding it (none at the top level) and the whole line the
/// found code starts on, whitespace collapsed. Lines added or removed above it, and edits to
/// other lines, leave it alone; editing its own line, or renaming the definition around it, makes
/// it a new finding.
///
/// Example: `std::fs::write("a.txt", x)` in `Order::save` and `std::fs::write("b.txt", y)` in
/// `Order::new` have different fingerprints, though the rule matched `std::fs::write` in both.
pub fn rule_finding_fingerprint(rule: &str, unit: Option<&str>, line: &str) -> String {
    fingerprint(rule, &format!("{} {line}", unit.unwrap_or_default()))
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

    #[test]
    fn a_rule_findings_fingerprint_tells_occurrences_apart() {
        let save = rule_finding_fingerprint("r", Some("Order::save"), "    write(\"a\");");
        assert_eq!(
            save,
            rule_finding_fingerprint("r", Some("Order::save"), "write(\"a\");")
        );
        assert_ne!(
            save,
            rule_finding_fingerprint("r", Some("Order::new"), "write(\"a\");")
        );
        assert_ne!(
            save,
            rule_finding_fingerprint("r", Some("Order::save"), "write(\"b\");")
        );
    }

    #[test]
    fn a_raw_finding_is_fingerprinted_by_its_whole_line() {
        let raw = RawFinding {
            rule: "r".into(),
            line: 2,
            node_text: "write".into(),
            kind: "k".into(),
            span: Span {
                start_row: 1,
                end_row: 1,
                start_byte: 4,
                end_byte: 9,
            },
            unit: Some("f".into()),
        };
        assert_eq!(
            raw.fingerprint("fn f() {\n    write(\"a\");\n}\n"),
            rule_finding_fingerprint("r", Some("f"), "write(\"a\");")
        );
    }
}
