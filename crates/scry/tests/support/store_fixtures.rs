//! The ports a test builds, and the finding store as a test reads and writes it.

use chrono::{DateTime, Utc};
use scry::adapters::json_finding_store::JsonFindingStore;
use scry::adapters::scry_toml::ScryToml;
use scry::domain::findings::Findings;
use scry::ports::finding_store::FindingStore;
use sorcerer_core::adapters::json_store::Snapshot;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::project::Project;

/// The ports a review talks through, over PROJECT's files.
pub struct Ports {
    pub store: JsonFindingStore,
    pub ledger: JsonlLedger,
    pub settings: ScryToml,
}

impl Ports {
    pub fn of(project: &Project) -> Ports {
        Ports {
            store: JsonFindingStore::for_project(project).unwrap(),
            ledger: JsonlLedger::for_project(project).unwrap(),
            settings: ScryToml::for_project(project),
        }
    }
}

/// PROJECT's finding store.
pub fn store_of(project: &Project) -> JsonFindingStore {
    JsonFindingStore::for_project(project).unwrap()
}

/// F applied to PROJECT's store under its lock; what F gave.
pub fn update_store<R>(
    project: &Project,
    f: impl FnOnce(&mut Findings) -> R,
) -> sorcerer_core::Result<R> {
    let mut f = Some(f);
    let mut out = None;
    store_of(project).update(&mut |s| out = f.take().map(|f| f(s)))?;
    Ok(out.expect("the update ran"))
}

/// PROJECT's findings as stored (none when there is no store yet).
pub fn read_store(project: &Project) -> Findings {
    match store_of(project).read().unwrap() {
        Snapshot::Current(s) => s,
        Snapshot::Absent => Findings::default(),
        Snapshot::Unreadable(why) => panic!("{why}"),
    }
}

/// An open finding as a pass would store it: ID, of ISSUE_TYPE, about FILE on LINE, in the
/// unit UNIT (a file-level finding when `None`) that started on UNIT_START and had the
/// fingerprint PRINT, citing its own line as evidence, with confidence 0.9.
pub fn open_finding(
    id: &str,
    issue_type: &str,
    file: &str,
    unit: Option<(&str, u32, &str)>,
    line: u32,
) -> scry::domain::finding::Finding {
    use scry::domain::finding::{Evidence, Finding, FindingId, Place, Scores, Status};
    use sorcerer_core::domain::fingerprint::Fingerprint;
    use sorcerer_core::domain::unit::Span;
    let t: DateTime<Utc> = "2026-11-20T12:00:00Z".parse().unwrap();
    Finding {
        id: FindingId(id.into()),
        issue_type: issue_type.into(),
        rule: None,
        statement: format!("statement of {id}"),
        place: Place {
            file: file.into(),
            unit: unit.map(|(name, _, _)| name.to_string()),
            lines: Span {
                start: line,
                end: line,
            },
            unit_start: unit.map(|(_, start, _)| start),
            fingerprint: unit.and_then(|(_, _, print)| Fingerprint::parse(print)),
        },
        evidence: vec![Evidence {
            file: file.into(),
            lines: Span {
                start: line,
                end: line,
            },
            print: None,
        }],
        scores: Scores {
            confidence: Some(0.9),
            asked_on: None,
        },
        verdict: None,
        status: Status::Open,
        seen: 1,
        first: t,
        last: t,
        history: vec![(t, Status::Open)],
        model: "sonnet".into(),
        cost_usd: 0.0,
    }
}

/// PROJECT's store holding exactly FINDINGS.
pub fn put_findings(project: &Project, findings: Vec<scry::domain::finding::Finding>) {
    update_store(project, |s| {
        *s = Findings {
            findings,
            ..Default::default()
        };
    })
    .unwrap();
}
