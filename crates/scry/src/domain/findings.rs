//! The findings a project holds: each finding, the units each file was
//! reviewed at, and the units a cap held back; and how a pass lays its
//! findings over the store as it is now.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::unit::Span;

use crate::domain::config::{MAX_SHOWN, ScryConfig};
use crate::domain::finding::{Finding, FindingId, Status};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Findings {
    pub findings: Vec<Finding>,
    /// Per file, the units a pass has looked at.
    pub reviewed: BTreeMap<PathBuf, BTreeSet<String>>,
    /// Per file, the units a cost cap held back.
    pub capped: BTreeMap<PathBuf, BTreeSet<String>>,
}

impl Findings {
    /// Six hex digits of the finding's hash; rehashed with a counter while taken.
    pub fn new_id(
        &self,
        at: DateTime<Utc>,
        file: &Path,
        lines: Span,
        statement: &str,
    ) -> FindingId {
        let base = format!(
            "{}\0{}\0{}\0{}\0{}",
            at.to_rfc3339(),
            file.display(),
            lines.start,
            lines.end,
            statement
        );
        let mut id = fnv64_hex(base.as_bytes())[..6].to_string();
        let mut n = 0;
        while self.findings.iter().any(|f| f.id.0 == id) {
            n += 1;
            id = fnv64_hex(format!("{base}\0{n}").as_bytes())[..6].to_string();
        }
        FindingId(id)
    }

    /// By id, or by a unique prefix of at least four characters.
    pub fn get(&self, id: &str) -> Option<&Finding> {
        self.index_of(id).ok()
    }

    fn index_of(&self, id: &str) -> std::result::Result<&Finding, String> {
        if let Some(f) = self.findings.iter().find(|f| f.id.0 == id) {
            return Ok(f);
        }
        if id.len() < 4 {
            return Err(format!("no finding {id}"));
        }
        let mut hits = self.findings.iter().filter(|f| f.id.0.starts_with(id));
        match (hits.next(), hits.next()) {
            (Some(f), None) => Ok(f),
            (Some(_), Some(_)) => Err(format!("{id} matches more than one finding")),
            _ => Err(format!("no finding {id}")),
        }
    }

    pub fn dismiss(
        &mut self,
        id: &str,
        everywhere: bool,
        at: DateTime<Utc>,
    ) -> std::result::Result<(), String> {
        let full = self.index_of(id)?.id.clone();
        let f = self
            .findings
            .iter_mut()
            .find(|f| f.id == full)
            .expect("found a moment ago");
        f.set_status(
            if everywhere {
                Status::DismissedEverywhere
            } else {
                Status::DismissedHere
            },
            at,
        );
        Ok(())
    }

    /// The file's open findings of shown issue types, best confidence first, at most `MAX_SHOWN`.
    pub fn shown_in(&self, cfg: &ScryConfig, file: &Path) -> Vec<&Finding> {
        let mut v: Vec<&Finding> = self
            .findings
            .iter()
            .filter(|f| {
                f.status == Status::Open
                    && f.place.file == file
                    && cfg.issue_types.contains(&f.issue_type)
            })
            .collect();
        v.sort_by(|a, b| {
            let (ka, kb) = (
                a.scores.confidence.unwrap_or(0.0),
                b.scores.confidence.unwrap_or(0.0),
            );
            kb.total_cmp(&ka)
        });
        v.truncate(MAX_SHOWN);
        v
    }
}

/// The pass's store (MINE, made from BASE) laid over CURRENT, the store as
/// it is now: a finding the pass changed takes the
/// pass's version only when its status in CURRENT is still what the pass
/// started from, so a dismissal made meanwhile stands; the pass's new
/// findings are added; reviewed sets are joined. A new finding whose id
/// was taken meanwhile (by a finding added from elsewhere) takes a fresh
/// one; RENAMES maps the pass's ids to the ids written, is applied to MINE
/// first and grows with each such clash. BASE is in written ids.
pub fn merge_into(
    base: &Findings,
    mine: &Findings,
    current: &Findings,
    renames: &mut HashMap<FindingId, FindingId>,
) -> Findings {
    let mine = renamed(mine, renames);
    let by_id = |s: &Findings| -> HashMap<FindingId, Finding> {
        s.findings
            .iter()
            .map(|f| (f.id.clone(), f.clone()))
            .collect()
    };
    let (b, m) = (by_id(base), by_id(&mine));
    let mut taken: HashSet<String> = current
        .findings
        .iter()
        .chain(&mine.findings)
        .map(|f| f.id.0.clone())
        .collect();
    let cur: HashSet<&FindingId> = current.findings.iter().map(|f| &f.id).collect();
    let mut findings: Vec<Finding> = current
        .findings
        .iter()
        .filter(|f| !(b.contains_key(&f.id) && !m.contains_key(&f.id)))
        .map(|f| match (b.get(&f.id), m.get(&f.id)) {
            (Some(was), Some(ours)) if was.status == f.status => ours.clone(),
            _ => f.clone(),
        })
        .collect();
    for f in mine.findings.iter().filter(|f| !b.contains_key(&f.id)) {
        let mut f = f.clone();
        if cur.contains(&f.id) {
            let id = free_id(&f.id, &taken);
            taken.insert(id.0.clone());
            renames.insert(f.id.clone(), id.clone());
            f.id = id;
        }
        findings.push(f);
    }
    let mut reviewed = current.reviewed.clone();
    for (file, set) in &mine.reviewed {
        reviewed
            .entry(file.clone())
            .or_default()
            .extend(set.iter().cloned());
    }
    let capped = if mine.capped == base.capped {
        current.capped.clone()
    } else {
        mine.capped.clone()
    };
    Findings {
        findings,
        reviewed,
        capped,
    }
}

/// STORE with its finding ids mapped through RENAMES.
pub fn renamed(store: &Findings, renames: &HashMap<FindingId, FindingId>) -> Findings {
    let mut s = store.clone();
    for f in &mut s.findings {
        if let Some(to) = renames.get(&f.id) {
            f.id = to.clone();
        }
    }
    s
}

/// Six hex digits from ID and a counter, not in TAKEN.
fn free_id(id: &FindingId, taken: &HashSet<String>) -> FindingId {
    (1..)
        .map(|n: u64| fnv64_hex(format!("{}\0{n}", id.0).as_bytes())[..6].to_string())
        .find(|c| !taken.contains(c))
        .map(FindingId)
        .expect("an unbounded counter finds a free id")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sorcerer_core::domain::fingerprint::Fingerprint;

    use crate::domain::finding::{Evidence, Place, Scores, StoredVerdict};

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn finding(id: &str, issue_type: &str, confidence: f64, status: Status) -> Finding {
        let t = at("2026-11-18T12:00:00Z");
        Finding {
            id: FindingId(id.into()),
            issue_type: issue_type.into(),
            rule: None,
            statement: format!("statement {id}"),
            place: Place {
                file: "a.txt".into(),
                unit: Some("f".into()),
                lines: Span { start: 1, end: 3 },
                unit_start: Some(1),
                fingerprint: Fingerprint::parse("0123abc"),
            },
            evidence: vec![Evidence {
                file: "a.txt".into(),
                lines: Span { start: 2, end: 2 },
                print: Some("0123abc".into()),
            }],
            scores: Scores {
                confidence: Some(confidence),
                asked_on: Some("h".into()),
            },
            verdict: Some(StoredVerdict {
                verdict: "right".into(),
                why: "w".into(),
                cited: Vec::new(),
            }),
            status,
            seen: 1,
            first: t,
            last: t,
            history: vec![(t, status)],
            model: "sonnet".into(),
            cost_usd: 0.04,
        }
    }

    #[test]
    fn ids_are_six_hex_and_rehash_on_collision() {
        let t = at("2026-11-18T12:00:00Z");
        let span = Span { start: 1, end: 2 };
        let mut s = Findings::default();
        let first = s.new_id(t, Path::new("a.txt"), span, "x");
        assert_eq!(first.0.len(), 6);
        assert!(first.0.chars().all(|c| c.is_ascii_hexdigit()));
        s.findings.push(finding(&first.0, "bug", 0.5, Status::Open));
        let second = s.new_id(t, Path::new("a.txt"), span, "x");
        assert_ne!(first, second);
        assert_eq!(second.0.len(), 6);
        assert!(second.0.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn dismiss_records_history_and_refuses_unknown_ids() {
        let mut s = Findings::default();
        s.findings.push(finding("abcdef", "bug", 0.9, Status::Open));
        let t = at("2026-11-19T00:00:00Z");
        s.dismiss("abcdef", false, t).unwrap();
        let f = s.get("abcdef").unwrap();
        assert_eq!(f.status, Status::DismissedHere);
        assert_eq!(f.history.last(), Some(&(t, Status::DismissedHere)));
        s.dismiss("abcd", true, t).unwrap();
        assert_eq!(s.findings[0].status, Status::DismissedEverywhere);
        assert!(s.dismiss("ffffff", false, t).is_err());
    }

    #[test]
    fn get_finds_by_a_unique_prefix_and_refuses_an_ambiguous_one() {
        let mut s = Findings::default();
        s.findings.push(finding("abcd01", "bug", 0.9, Status::Open));
        s.findings.push(finding("abcd02", "bug", 0.9, Status::Open));
        s.findings.push(finding("ffee11", "bug", 0.9, Status::Open));
        assert!(s.get("abcd").is_none());
        assert!(
            s.dismiss("abcd", false, at("2026-11-19T00:00:00Z"))
                .is_err()
        );
        assert_eq!(s.get("abcd02").unwrap().id.0, "abcd02");
        assert_eq!(s.get("ffee").unwrap().id.0, "ffee11");
        assert!(s.get("ff").is_none());
    }

    #[test]
    fn shown_in_keeps_the_best_open_of_shown_issue_types() {
        let mut s = Findings::default();
        s.findings
            .push(finding("000001", "bug", 0.75, Status::Open));
        s.findings
            .push(finding("000002", "design", 0.99, Status::Open));
        s.findings
            .push(finding("000003", "convention", 0.95, Status::Open));
        s.findings
            .push(finding("000004", "assumption", 0.85, Status::Open));
        s.findings.push(finding("000005", "bug", 0.8, Status::Open));
        s.findings
            .push(finding("000006", "bug", 0.99, Status::Withheld));
        let cfg = ScryConfig::default();
        let ids: Vec<&str> = s
            .shown_in(&cfg, Path::new("a.txt"))
            .iter()
            .map(|f| f.id.0.as_str())
            .collect();
        assert_eq!(ids, ["000003", "000004", "000005"]);
        assert!(s.shown_in(&cfg, Path::new("b.txt")).is_empty());
    }
}
