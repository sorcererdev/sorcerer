//! The finding store as `.sorcerer/state/scry/findings.json`, version 2,
//! over core's versioned store; the pass lock is `pass.lock` beside it.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use sorcerer_core::adapters::json_store::{Snapshot, Store, Versioned};
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::findings::Findings;
use crate::ports::finding_store::{FindingStore, Held};

impl Versioned for Findings {
    const VERSION: u32 = 2;
}

pub struct JsonFindingStore {
    store: Store<Findings>,
}

impl JsonFindingStore {
    /// The project's store (its state directory is created).
    pub fn for_project(p: &Project) -> Result<JsonFindingStore> {
        Ok(JsonFindingStore {
            store: Store::in_project(p, "scry/findings.json")?,
        })
    }

    /// The store at PATH, creating nothing until it is written.
    pub fn at(path: PathBuf) -> JsonFindingStore {
        JsonFindingStore {
            store: Store::at(path),
        }
    }
}

struct Lock(#[allow(dead_code)] File);

impl Held for Lock {}

impl FindingStore for JsonFindingStore {
    fn read(&self) -> Result<Snapshot<Findings>> {
        self.store.read()
    }

    fn update(&self, f: &mut dyn FnMut(&mut Findings)) -> Result<()> {
        self.store.update(|s| f(s))
    }

    fn hold(&self) -> Result<Box<dyn Held>> {
        let dir = self.path().parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let lock_path = dir.join("pass.lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| Error::io(&lock_path, e))?;
        lock.lock().map_err(|e| Error::io(&lock_path, e))?;
        Ok(Box::new(Lock(lock)))
    }

    fn path(&self) -> &Path {
        self.store.path()
    }
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use chrono::{DateTime, Utc};
    use sorcerer_core::domain::fingerprint::Fingerprint;
    use sorcerer_core::domain::unit::Span;

    use crate::domain::finding::{
        Evidence, Finding, FindingId, Place, Scores, Status, StoredVerdict,
    };

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
        Status::Unverified,
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
        let st: Store<Findings> = Store::at(dir.path().join("scry/findings.json"));
        let mut data = Findings::default();
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
        let st: Store<Findings> = Store::at(path.clone());
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
    fn a_store_from_a_newer_scry_is_unreadable_and_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("findings.json");
        let bytes = br#"{"version": 3, "data": {}}"#;
        std::fs::write(&path, bytes).unwrap();
        let st: Store<Findings> = Store::at(path.clone());
        assert!(matches!(st.read().unwrap(), Snapshot::Unreadable(_)));
        assert!(st.update(|_| ()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
