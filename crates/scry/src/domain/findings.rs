//! The findings a project holds, and how a pass writes its work into them.
//!
//! The store keeps every finding scry has raised, which units of each file
//! a pass has reviewed, and which units a cost cap held back. A pass works
//! on its own copy; [`merge_into`] writes that copy over the store as it is
//! now, so a dismissal the user made meanwhile is not lost.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::Span;

use crate::domain::config::{MAX_SHOWN, ScryConfig};
use crate::domain::finding::{Dismissal, Finding, FindingId, Status};
use crate::domain::scope::Scope;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Findings {
    pub findings: Vec<Finding>,
    /// For each file, the fingerprints of the unit versions a pass has
    /// reviewed: a unit at one of them is not sent again.
    pub reviewed: BTreeMap<PathBuf, BTreeSet<String>>,
    /// For each file, the names of the units a cost cap kept from review.
    pub capped: BTreeMap<PathBuf, BTreeSet<String>>,
}

impl Findings {
    /// A new finding's id: six hex digits hashed from when it was raised,
    /// its file, its lines and its statement. If another finding already has
    /// those digits, the hash is taken again with a counter added (1, 2, ...)
    /// until the digits are free.
    ///
    /// Example: `"3fa9c0"`.
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
            forward_slashes(file),
            lines.start,
            lines.end,
            statement
        );
        let with_counter = (1..).map(|n: u64| format!("{base}\0{n}"));
        std::iter::once(base.clone())
            .chain(with_counter)
            .map(|text| six_hex_digits(&text))
            .find(|id| !self.has_id(id))
            .map(FindingId)
            .expect("an unbounded counter finds a free id")
    }

    fn has_id(&self, id: &str) -> bool {
        self.findings.iter().any(|f| f.id.0 == id)
    }

    /// The finding with this id, or with an id that starts with it, as long
    /// as the start is at least four characters and only one finding has it.
    ///
    /// Example: with findings `abcd01`, `abcd02` and `ffee11`, `"ffee"`
    /// finds `ffee11`, while `"abcd"` (two match) and `"ff"` (too short)
    /// find nothing.
    pub fn get(&self, id: &str) -> Option<&Finding> {
        self.find(id).ok()
    }

    /// The finding [`Findings::get`] would find, or why there is none, in
    /// words the user is shown: `no finding ff` or `abcd matches more than
    /// one finding`.
    pub fn find(&self, id: &str) -> std::result::Result<&Finding, String> {
        self.position_of(id).map(|i| &self.findings[i])
    }

    /// Where the finding [`Findings::get`] would find is in the list, or
    /// why there is none, in words the user is shown.
    fn position_of(&self, id: &str) -> std::result::Result<usize, String> {
        if let Some(i) = self.findings.iter().position(|f| f.id.0 == id) {
            return Ok(i);
        }
        if id.len() < 4 {
            return Err(format!("no finding {id}"));
        }
        let mut starting_with_id = self
            .findings
            .iter()
            .enumerate()
            .filter(|(_, f)| f.id.0.starts_with(id))
            .map(|(i, _)| i);
        match (starting_with_id.next(), starting_with_id.next()) {
            (Some(i), None) => Ok(i),
            (Some(_), Some(_)) => Err(format!("{id} matches more than one finding")),
            _ => Err(format!("no finding {id}")),
        }
    }

    /// Marks a finding dismissed, here or everywhere, found as
    /// [`Findings::get`] finds it. The change goes into its history.
    /// An id that finds no finding is an error saying so.
    pub fn dismiss(
        &mut self,
        id: &str,
        dismissal: Dismissal,
        at: DateTime<Utc>,
    ) -> std::result::Result<(), String> {
        let i = self.position_of(id)?;
        self.findings[i].set_status(dismissal.status(), at);
        Ok(())
    }

    /// The findings a review prints: every open finding of the given issue
    /// types whose unit is in the scope, the most confident first. There is
    /// no limit per file here: the pass already showed at most 3 in each.
    pub fn shown_in_scope(&self, scope: &Scope, issue_types: &[String]) -> Vec<&Finding> {
        let in_scope = |f: &&Finding| {
            scope.units.iter().any(|scoped| {
                scoped.file == f.place.file
                    && f.place.unit.as_deref() == Some(scoped.unit.name.as_str())
            })
        };
        let open_in_scope = self
            .findings
            .iter()
            .filter(|f| is_open_of(f, issue_types))
            .filter(in_scope);
        most_confident_first(open_in_scope.collect())
    }

    /// The findings shown in one file: its open findings of the issue types
    /// the project shows, the most confident first, at most 3.
    pub fn shown_in(&self, cfg: &ScryConfig, file: &Path) -> Vec<&Finding> {
        let in_file = self
            .findings
            .iter()
            .filter(|f| is_open_of(f, &cfg.issue_types))
            .filter(|f| f.place.file == file)
            .collect();
        let mut shown = most_confident_first(in_file);
        shown.truncate(MAX_SHOWN);
        shown
    }

    /// The open and unverified findings on a unit put back to the base's code: the scope says
    /// the unit is the same now as at the base, at a fingerprint other than the one the
    /// finding was made on. A finding stored with no fingerprint is left alone.
    ///
    /// Example: you saved a change to `cancel`, a pass raised a finding on it at 0e41bd1, and
    /// you undid the change. `cancel` is what `HEAD` has again (7c2a9f0), so no pass would
    /// review it: the finding is about code that is no longer there. Had you committed the
    /// change instead, `HEAD` would hold 0e41bd1, and the finding would not be put back.
    pub fn put_back(&self, scope: &Scope) -> Vec<FindingId> {
        let same_as_base = |f: &Finding| {
            scope.files.iter().any(|sf| {
                sf.file == f.place.file
                    && f.place.unit.as_ref().is_some_and(|name| {
                        sf.same.iter().any(|(same, at)| {
                            same == name
                                && f.place.fingerprint.is_some_and(|made_on| made_on != *at)
                        })
                    })
            })
        };
        self.findings
            .iter()
            .filter(|f| matches!(f.status, Status::Open | Status::Unverified))
            .filter(|f| same_as_base(f))
            .map(|f| f.id.clone())
            .collect()
    }

    /// Closes each finding in `ids` (resolved or gone, dated `at`) because its code changed,
    /// and forgets that the code it was made on was reviewed: if the unit comes back to that
    /// code, a pass sends it again rather than counting it as reviewed with nothing to say.
    ///
    /// Example: a finding on `cancel` at fingerprint 0e41bd1 is resolved because you put
    /// `cancel` back as the base has it. You then redo the change: `cancel` is at 0e41bd1
    /// again, and the next pass sends it, so the finding shows again.
    pub fn close_for_changed_code(&mut self, ids: &[FindingId], status: Status, at: DateTime<Utc>) {
        let made_on: Vec<(PathBuf, String)> = self
            .findings
            .iter()
            .filter(|f| ids.contains(&f.id))
            .filter_map(|f| Some((f.place.file.clone(), f.place.fingerprint?.to_string())))
            .collect();
        for (file, print) in made_on {
            if let Some(prints) = self.reviewed.get_mut(&file) {
                prints.remove(&print);
            }
        }
        self.reviewed.retain(|_, prints| !prints.is_empty());
        for f in self.findings.iter_mut().filter(|f| ids.contains(&f.id)) {
            f.set_status(status, at);
        }
    }
}

fn is_open_of(f: &Finding, issue_types: &[String]) -> bool {
    f.status == Status::Open && issue_types.contains(&f.issue_type)
}

/// Sorted by confidence, highest first; a finding with no confidence counts
/// as 0. Findings with the same confidence keep their order.
fn most_confident_first(mut findings: Vec<&Finding>) -> Vec<&Finding> {
    let confidence = |f: &Finding| f.scores.confidence.unwrap_or(0.0);
    findings.sort_by(|a, b| confidence(b).total_cmp(&confidence(a)));
    findings
}

/// The store after a pass writes its working copy over it.
///
/// The user may have changed the store while the pass ran (dismissed a
/// finding, say), so the pass does not simply overwrite it. It compares its
/// working copy with what it last wrote, and changes only what it changed
/// itself:
///
/// 1. A finding the pass removed is removed.
/// 2. A finding the pass changed takes the pass's version, but only if its
///    status in the store is still the one the pass last wrote. Otherwise
///    someone else changed it, and their change stands.
/// 3. The pass's new findings are added. If one's id was taken meanwhile by
///    a finding added from elsewhere, it is given a free id, and
///    `renamed_ids` records the change so the pass's later writes use it.
/// 4. The reviewed fingerprints are the store's, with those the pass added
///    and less those the pass forgot (a finding on that code was resolved
///    or gone, so the code is reviewed again if it comes back).
/// 5. The capped units are the pass's if it changed them, else the store's.
///
/// `renamed_ids` is applied to the working copy first; `last_written` is
/// already in the ids written.
///
/// Example: the pass raised its confidence in finding `3fa9c0` while the
/// user dismissed it. The store keeps the dismissal.
pub fn merge_into(
    last_written: &Findings,
    working_copy: &Findings,
    current: &Findings,
    renamed_ids: &mut HashMap<FindingId, FindingId>,
) -> Findings {
    let working_copy = renamed(working_copy, renamed_ids);
    let pass = PassEdits::between(last_written, &working_copy);
    let kept: Vec<Finding> = current
        .findings
        .iter()
        .filter(|f| !pass.removed(f))
        .map(|f| pass.version_to_keep(f))
        .collect();
    let added = pass.new_findings_under_free_ids(current, renamed_ids);
    Findings {
        findings: kept.into_iter().chain(added).collect(),
        reviewed: pass.reviewed_after(current),
        capped: pass.capped_units(current),
    }
}

/// What a pass changed: the store as it last wrote it, and its working
/// copy now, each finding by id.
struct PassEdits<'a> {
    last_written: &'a Findings,
    working_copy: &'a Findings,
    before: HashMap<&'a FindingId, &'a Finding>,
    after: HashMap<&'a FindingId, &'a Finding>,
}

impl<'a> PassEdits<'a> {
    fn between(last_written: &'a Findings, working_copy: &'a Findings) -> Self {
        let by_id = |store: &'a Findings| store.findings.iter().map(|f| (&f.id, f)).collect();
        PassEdits {
            last_written,
            working_copy,
            before: by_id(last_written),
            after: by_id(working_copy),
        }
    }

    fn removed(&self, f: &Finding) -> bool {
        self.before.contains_key(&f.id) && !self.after.contains_key(&f.id)
    }

    /// The pass's version of a finding if nobody else changed its status
    /// since the pass last wrote it, else the store's.
    fn version_to_keep(&self, in_store: &Finding) -> Finding {
        match (self.before.get(&in_store.id), self.after.get(&in_store.id)) {
            (Some(was), Some(ours)) if was.status == in_store.status => (*ours).clone(),
            _ => in_store.clone(),
        }
    }

    /// The findings the pass raised since it last wrote, each under an id
    /// no other finding has. Each id changed is recorded in `renamed_ids`.
    fn new_findings_under_free_ids(
        &self,
        current: &Findings,
        renamed_ids: &mut HashMap<FindingId, FindingId>,
    ) -> Vec<Finding> {
        let in_store: HashSet<&FindingId> = current.findings.iter().map(|f| &f.id).collect();
        let mut taken: HashSet<String> = current
            .findings
            .iter()
            .chain(&self.working_copy.findings)
            .map(|f| f.id.0.clone())
            .collect();
        let new = self
            .working_copy
            .findings
            .iter()
            .filter(|f| !self.before.contains_key(&f.id));
        new.map(|f| {
            if !in_store.contains(&f.id) {
                return f.clone();
            }
            let free = free_id(&f.id, &taken);
            taken.insert(free.0.clone());
            renamed_ids.insert(f.id.clone(), free.clone());
            Finding {
                id: free,
                ..f.clone()
            }
        })
        .collect()
    }

    /// The store's reviewed fingerprints with the pass's edits laid over
    /// them: what it marked reviewed since it last wrote is added, what it
    /// forgot is taken out, and what another writer marked meanwhile stays.
    fn reviewed_after(&self, current: &Findings) -> BTreeMap<PathBuf, BTreeSet<String>> {
        let added = fingerprints_only_in(&self.working_copy.reviewed, &self.last_written.reviewed);
        let forgotten =
            fingerprints_only_in(&self.last_written.reviewed, &self.working_copy.reviewed);
        let files = current.reviewed.keys().chain(added.keys());
        files
            .map(|file| {
                let kept_or_added: BTreeSet<String> = current
                    .reviewed
                    .get(file)
                    .into_iter()
                    .chain(added.get(file))
                    .flatten()
                    .filter(|print| !forgotten.get(file).is_some_and(|f| f.contains(*print)))
                    .cloned()
                    .collect();
                (file.clone(), kept_or_added)
            })
            .filter(|(_, prints)| !prints.is_empty())
            .collect()
    }

    fn capped_units(&self, current: &Findings) -> BTreeMap<PathBuf, BTreeSet<String>> {
        if self.working_copy.capped == self.last_written.capped {
            current.capped.clone()
        } else {
            self.working_copy.capped.clone()
        }
    }
}

/// For each file, the fingerprints in `these` that `those` does not have.
fn fingerprints_only_in(
    these: &BTreeMap<PathBuf, BTreeSet<String>>,
    those: &BTreeMap<PathBuf, BTreeSet<String>>,
) -> BTreeMap<PathBuf, BTreeSet<String>> {
    these
        .iter()
        .map(|(file, prints)| {
            let only_here = match those.get(file) {
                Some(theirs) => prints.difference(theirs).cloned().collect(),
                None => prints.clone(),
            };
            (file.clone(), only_here)
        })
        .filter(|(_, prints): &(PathBuf, BTreeSet<String>)| !prints.is_empty())
        .collect()
}

/// The store with each finding id that `renamed_ids` lists replaced by the
/// id it was renamed to.
pub fn renamed(store: &Findings, renamed_ids: &HashMap<FindingId, FindingId>) -> Findings {
    let findings = store
        .findings
        .iter()
        .map(|f| match renamed_ids.get(&f.id) {
            Some(to) => Finding {
                id: to.clone(),
                ..f.clone()
            },
            None => f.clone(),
        })
        .collect();
    Findings {
        findings,
        ..store.clone()
    }
}

/// A fresh six-hex-digit id for a finding whose id is taken: the hash of
/// its id with a counter added (1, 2, ...), the first one not in `taken`.
fn free_id(id: &FindingId, taken: &HashSet<String>) -> FindingId {
    (1..)
        .map(|n: u64| six_hex_digits(&format!("{}\0{n}", id.0)))
        .find(|candidate| !taken.contains(candidate))
        .map(FindingId)
        .expect("an unbounded counter finds a free id")
}

fn six_hex_digits(text: &str) -> String {
    fnv64_hex(text.as_bytes())[..6].to_string()
}

#[allow(clippy::too_many_lines)]
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
        s.dismiss("abcdef", Dismissal::Here, t).unwrap();
        let f = s.get("abcdef").unwrap();
        assert_eq!(f.status, Status::DismissedHere);
        assert_eq!(f.history.last(), Some(&(t, Status::DismissedHere)));
        s.dismiss("abcd", Dismissal::Everywhere, t).unwrap();
        assert_eq!(s.findings[0].status, Status::DismissedEverywhere);
        assert!(s.dismiss("ffffff", Dismissal::Here, t).is_err());
    }

    #[test]
    fn get_finds_by_a_unique_prefix_and_refuses_an_ambiguous_one() {
        let mut s = Findings::default();
        s.findings.push(finding("abcd01", "bug", 0.9, Status::Open));
        s.findings.push(finding("abcd02", "bug", 0.9, Status::Open));
        s.findings.push(finding("ffee11", "bug", 0.9, Status::Open));
        assert!(s.get("abcd").is_none());
        assert!(
            s.dismiss("abcd", Dismissal::Here, at("2026-11-19T00:00:00Z"))
                .is_err()
        );
        assert_eq!(s.get("abcd02").unwrap().id.0, "abcd02");
        assert_eq!(s.get("ffee").unwrap().id.0, "ffee11");
        assert!(s.get("ff").is_none());
    }

    #[test]
    fn shown_in_scope_is_open_findings_of_shown_issue_types_in_scope_units_best_first() {
        use crate::domain::scope::{Scope, ScopeRequest, ScopedUnit};
        use sorcerer_core::domain::unit::{Unit, UnitKind};
        let unit = |name: &str| ScopedUnit {
            file: "a.txt".into(),
            unit: Unit {
                name: name.into(),
                kind: UnitKind::Function,
                span: Span { start: 1, end: 3 },
                body: None,
                fingerprint: Fingerprint::parse("0123abc").unwrap(),
            },
            header: None,
        };
        let scope = Scope {
            request: ScopeRequest::Working,
            base: crate::domain::scope::Base::Revision("HEAD".into()),
            units: vec![unit("f")],
            files: vec![],
            skipped: vec![],
        };
        let mut s = Findings::default();
        s.findings
            .push(finding("000001", "bug", 0.75, Status::Open));
        s.findings
            .push(finding("000002", "design", 0.99, Status::Open));
        s.findings
            .push(finding("000003", "bug", 0.95, Status::Open));
        s.findings
            .push(finding("000004", "bug", 0.99, Status::Withheld));
        let mut elsewhere = finding("000005", "bug", 0.99, Status::Open);
        elsewhere.place.unit = Some("g".into());
        s.findings.push(elsewhere);
        for n in 6..10 {
            s.findings
                .push(finding(&format!("00000{n}"), "bug", 0.7, Status::Open));
        }
        let ids: Vec<&str> = s
            .shown_in_scope(&scope, &ScryConfig::default().issue_types)
            .iter()
            .map(|f| f.id.0.as_str())
            .collect();
        // no MAX_SHOWN here: every open finding in scope is printed
        assert_eq!(
            ids,
            ["000003", "000001", "000006", "000007", "000008", "000009"]
        );
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
