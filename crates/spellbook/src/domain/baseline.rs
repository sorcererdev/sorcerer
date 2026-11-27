//! The baseline: the findings a project has decided to live with for now, recorded by
//! `spellbook check --baseline` in `.sorcerer/spellbook/baseline.toml`. A baselined finding is
//! shown as existing and never fails `check`; a new one does. Debt goes down as people fix the
//! code and record the baseline again.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::finding::Finding;

/// The first lines of `baseline.toml`, saying what it is and how it changes.
const HEADER: &str = "\
# Findings recorded by `spellbook check --baseline`: known debt. `spellbook check` shows them as
# existing and fails only on findings not listed here. Re-run --baseline as the debt is paid.
";

/// One recorded finding: its rule, its file and its fingerprint (what it is about, not its line).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineEntry {
    pub file: String,
    pub rule: String,
    pub fingerprint: String,
}

/// The recorded findings, sorted by file, rule and fingerprint. An entry stands for one finding:
/// two identical findings in a file are two entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Baseline {
    entries: Vec<BaselineEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselineFile {
    #[serde(default)]
    baseline: Vec<BaselineEntry>,
}

/// How a newer baseline differs from an older one, in findings.
///
/// Example: an older baseline of 3 findings, one fixed since and one new added, is `{ paid_off: 1,
/// new: 1 }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaselineChange {
    pub paid_off: usize,
    pub new: usize,
}

/// The baseline in the text of a `baseline.toml`; the problem is TOML's sentence.
///
/// Example: `[[baseline]]\nrule = "r"\nfile = "a.x"\nfingerprint = "3fa9c01"\n` is one entry.
pub fn parse_baseline(text: &str) -> Result<Baseline, String> {
    let file: BaselineFile =
        toml::from_str(text).map_err(|e: toml::de::Error| e.message().trim().to_string())?;
    Ok(Baseline::sorted(file.baseline))
}

/// The text of `baseline.toml` for a baseline: the header, then one `[[baseline]]` table each.
pub fn baseline_text(baseline: &Baseline) -> String {
    let body = toml::to_string(&BaselineFile {
        baseline: baseline.entries.clone(),
    })
    .unwrap_or_default();
    format!("{HEADER}\n{body}")
}

impl Baseline {
    fn sorted(mut entries: Vec<BaselineEntry>) -> Baseline {
        entries.sort();
        Baseline { entries }
    }

    /// The baseline of today's findings: every rule's finding, none of the regions' (a region's
    /// fix is `spellbook sync`, so it is never debt).
    pub fn of(findings: &[Finding]) -> Baseline {
        Baseline::sorted(
            findings
                .iter()
                .filter(|finding| finding.region.is_none())
                .map(entry_of)
                .collect(),
        )
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The findings with `existing` set on each one an entry stands for, each entry used once,
    /// in the findings' order.
    ///
    /// Example: one entry for `import sqlite3` in `a.x` and two such findings in `a.x`: the
    /// first is existing, the second is new.
    pub fn marked(&self, findings: Vec<Finding>) -> Vec<Finding> {
        self.marked_unchanged_first(findings, |_| true)
    }

    /// The findings marked as [`Baseline::marked`] does, but the entries go to the findings
    /// `unchanged` says are old code before any other: of two identical findings, the one on a
    /// line a branch did not touch is the existing one, and its copy is new.
    ///
    /// Example: one entry for `write(x)` in `a.x`, found on line 3 (untouched) and line 9
    /// (pasted on the branch): line 3 is existing and line 9 is new, whichever comes first.
    pub fn marked_unchanged_first(
        &self,
        findings: Vec<Finding>,
        unchanged: impl Fn(&Finding) -> bool,
    ) -> Vec<Finding> {
        let mut left = self.counts();
        let mut existing = vec![false; findings.len()];
        let old_first = (0..findings.len())
            .filter(|&at| unchanged(&findings[at]))
            .chain((0..findings.len()).filter(|&at| !unchanged(&findings[at])));
        for at in old_first {
            let finding = &findings[at];
            existing[at] = finding.region.is_none() && take_one(&mut left, &entry_of(finding));
        }
        findings
            .into_iter()
            .zip(existing)
            .map(|(finding, existing)| Finding {
                existing,
                ..finding
            })
            .collect()
    }

    /// What `newer` pays off and adds compared with this baseline.
    pub fn change_to(&self, newer: &Baseline) -> BaselineChange {
        BaselineChange {
            paid_off: missing_from(self, newer),
            new: missing_from(newer, self),
        }
    }

    fn counts(&self) -> BTreeMap<&BaselineEntry, usize> {
        self.entries
            .iter()
            .fold(BTreeMap::new(), |mut counts, entry| {
                *counts.entry(entry).or_default() += 1;
                counts
            })
    }
}

/// How many of `from`'s entries `to` has no entry left for.
fn missing_from(from: &Baseline, to: &Baseline) -> usize {
    let mut left = to.counts();
    from.entries
        .iter()
        .filter(|entry| !take_one(&mut left, entry))
        .count()
}

fn take_one(left: &mut BTreeMap<&BaselineEntry, usize>, entry: &BaselineEntry) -> bool {
    match left.get_mut(entry) {
        Some(count) if *count > 0 => {
            *count -= 1;
            true
        }
        _ => false,
    }
}

fn entry_of(finding: &Finding) -> BaselineEntry {
    BaselineEntry {
        file: finding.file.clone(),
        rule: finding.rule.clone(),
        fingerprint: finding.fingerprint.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::rule::Severity;

    fn finding(file: &str, fingerprint: &str) -> Finding {
        Finding {
            rule: "r".into(),
            file: file.into(),
            line: 1,
            severity: Severity::Error,
            message: "m".into(),
            fingerprint: fingerprint.into(),
            node_text: String::new(),
            region: None,
            existing: false,
        }
    }

    fn existing(found: &[Finding]) -> Vec<bool> {
        found.iter().map(|finding| finding.existing).collect()
    }

    #[test]
    fn each_entry_stands_for_one_finding() {
        let baseline = Baseline::of(&[finding("a.x", "1111111")]);
        let found = baseline.marked(vec![
            finding("a.x", "1111111"),
            finding("a.x", "1111111"),
            finding("b.x", "1111111"),
        ]);
        assert_eq!(existing(&found), [true, false, false]);
    }

    #[test]
    fn unchanged_findings_take_the_entries_first() {
        let baseline = Baseline::of(&[finding("a.x", "1111111")]);
        let pasted_above = vec![
            finding("a.x", "1111111"),
            Finding {
                line: 2,
                ..finding("a.x", "1111111")
            },
        ];
        let found = baseline.marked_unchanged_first(pasted_above, |f| f.line == 2);
        assert_eq!(existing(&found), [false, true]);
    }

    #[test]
    fn the_text_reads_back_sorted() {
        let baseline = Baseline::of(&[finding("b.x", "2222222"), finding("a.x", "1111111")]);
        let text = baseline_text(&baseline);
        assert!(text.starts_with("# Findings recorded"), "{text}");
        assert!(text.find("a.x") < text.find("b.x"), "{text}");
        assert_eq!(parse_baseline(&text).unwrap(), baseline);
    }

    #[test]
    fn a_change_counts_paid_off_and_new() {
        let older = Baseline::of(&[
            finding("a.x", "1"),
            finding("a.x", "2"),
            finding("a.x", "2"),
        ]);
        let newer = Baseline::of(&[finding("a.x", "2"), finding("a.x", "3")]);
        assert_eq!(
            older.change_to(&newer),
            BaselineChange {
                paid_off: 2,
                new: 1
            }
        );
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let why = parse_baseline(
            "[[baseline]]\nrule = \"r\"\nfile = \"a\"\nfingerprint = \"1\"\nline = 3\n",
        )
        .unwrap_err();
        assert!(why.contains("line"), "{why}");
    }
}
