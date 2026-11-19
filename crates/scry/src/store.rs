//! The finding store: `.sorcerer/state/scry/findings.json`, version 2.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::Result;
use sorcerer_core::adapters::json_store::{Store, Versioned};
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Span;

use crate::config::{MAX_SHOWN, ScryConfig};
pub use crate::finding::{
    Evidence, Finding, FindingId, Place, Scores, Status, StoredCitation, StoredVerdict,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScryStore {
    pub findings: Vec<Finding>,
    /// Per file, the units a pass has looked at.
    pub reviewed: BTreeMap<PathBuf, BTreeSet<String>>,
    /// Per file, the units a cost cap held back.
    pub capped: BTreeMap<PathBuf, BTreeSet<String>>,
}

impl Versioned for ScryStore {
    const VERSION: u32 = 2;
}

pub fn store(p: &Project) -> Result<Store<ScryStore>> {
    Store::in_project(p, "scry/findings.json")
}

impl ScryStore {
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

#[cfg(test)]
mod tests {
    use super::*;
    use sorcerer_core::adapters::json_store::Snapshot;
    use sorcerer_core::domain::fingerprint::Fingerprint;

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

    const ALL: [Status; 10] = [
        Status::Open,
        Status::Waiting,
        Status::Withheld,
        Status::Refuted,
        Status::Unsure,
        Status::Elsewhere,
        Status::DismissedHere,
        Status::DismissedEverywhere,
        Status::Resolved,
        Status::Gone,
    ];

    #[test]
    fn a_store_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let st: Store<ScryStore> = Store::at(dir.path().join("scry/findings.json"));
        let mut data = ScryStore::default();
        for (i, s) in ALL.iter().enumerate() {
            data.findings
                .push(finding(&format!("00000{i}"), "bug", 0.9, *s));
        }
        data.reviewed
            .insert("a.txt".into(), BTreeSet::from(["f".to_string()]));
        data.capped
            .insert("b.txt".into(), BTreeSet::from(["g".to_string()]));
        st.update(|s| *s = data.clone()).unwrap();
        match st.read().unwrap() {
            Snapshot::Current(back) => assert_eq!(back, data),
            other => panic!("{other:?}"),
        }
        let text = std::fs::read_to_string(st.path()).unwrap();
        assert!(text.contains("\"status\": \"dismissed-here\""), "{text}");
        for word in ["\"issue_type\"", "\"confidence\"", "\"reviewed\""] {
            assert!(text.contains(word), "{word} missing: {text}");
        }
        for word in ["\"kind\"", "\"keep\"", "\"inspected\""] {
            assert!(!text.contains(word), "{word} still there: {text}");
        }
    }

    #[test]
    fn a_version_one_store_is_unreadable_and_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("findings.json");
        let bytes = br#"{"version": 1, "data": {"findings": [], "inspected": {}, "capped": {}}}"#;
        std::fs::write(&path, bytes).unwrap();
        let st: Store<ScryStore> = Store::at(path.clone());
        match st.read().unwrap() {
            Snapshot::Unreadable(why) => {
                assert_eq!(why, "version 1, and this sorcerer reads version 2")
            }
            other => panic!("{other:?}"),
        }
        assert!(st.update(|_| ()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn ids_are_six_hex_and_rehash_on_collision() {
        let t = at("2026-11-18T12:00:00Z");
        let span = Span { start: 1, end: 2 };
        let mut s = ScryStore::default();
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
        let mut s = ScryStore::default();
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
        let mut s = ScryStore::default();
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
        let mut s = ScryStore::default();
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

    #[test]
    fn a_store_from_a_newer_scry_is_unreadable_and_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("findings.json");
        let bytes = br#"{"version": 3, "data": {}}"#;
        std::fs::write(&path, bytes).unwrap();
        let st: Store<ScryStore> = Store::at(path.clone());
        assert!(matches!(st.read().unwrap(), Snapshot::Unreadable(_)));
        assert!(st.update(|_| ()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
