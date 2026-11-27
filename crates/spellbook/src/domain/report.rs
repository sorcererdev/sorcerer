//! `report.json`: the findings of the last check.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::versioned::Versioned;

use crate::domain::finding::Finding;

/// The last check: when, and what it found, by file then line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportFile {
    /// When the report was last written; none before the first check.
    pub checked: Option<DateTime<Utc>>,
    pub findings: Vec<Finding>,
}

impl Versioned for ReportFile {
    const VERSION: u32 = 1;
}

impl ReportFile {
    /// The report with the findings of the given files replaced by new ones, and the time
    /// updated.
    ///
    /// Example: a report with findings in `a.txt` and `b.txt`, replaced for `a.txt` with none, keeps
    /// only `b.txt`'s.
    pub fn with_files_replaced(
        mut self,
        files: &[String],
        found: Vec<Finding>,
        at: DateTime<Utc>,
    ) -> ReportFile {
        self.findings
            .retain(|finding| !files.contains(&finding.file));
        self.findings.extend(found);
        sort_findings(&mut self.findings);
        self.checked = Some(at);
        self
    }
}

/// By file, then line, then rule.
pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| (&a.file, a.line, &a.rule).cmp(&(&b.file, b.line, &b.rule)));
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::domain::rule::Severity;

    fn finding(file: &str, line: u32) -> Finding {
        Finding {
            region: None,
            rule: "r".into(),
            file: file.into(),
            line,
            severity: Severity::Warn,
            message: "m".into(),
            fingerprint: "0000000".into(),
            node_text: String::new(),
            existing: false,
        }
    }

    #[test]
    fn replacing_a_files_findings_keeps_the_others() {
        let at = Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap();
        let report = ReportFile {
            checked: None,
            findings: vec![finding("a.txt", 1), finding("b.txt", 2)],
        };
        let after =
            report.with_files_replaced(&["a.txt".to_string()], vec![finding("a.txt", 9)], at);
        let lines: Vec<(&str, u32)> = after
            .findings
            .iter()
            .map(|f| (f.file.as_str(), f.line))
            .collect();
        assert_eq!(lines, [("a.txt", 9), ("b.txt", 2)]);
        assert_eq!(after.checked, Some(at));
    }
}
