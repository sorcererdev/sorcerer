//! Remembering what a review settled, and which units are now reviewed.
//!
//! After the findings are judged, `remember` resolves the findings the
//! reviewer said no longer hold, drops the new findings of units the user
//! edited while scry was looking at them, and, only when every unit was
//! reviewed, resolves the findings the reviewer did not raise again in a
//! unit whose code changed (they were fixed). Findings whose unit was
//! deleted were already found by `sweep_gone`, before the review, and
//! findings on a unit put back to the base's code by `sweep_put_back`.
//!
//! After the paid checks, `mark_reviewed` records each unit the reviewer
//! answered for as reviewed at the version it saw, so the next run does
//! not send it again until it changes.
//!
//! The run log's text is built last in the file.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::Unit;

use super::ask::ReviewResults;
use super::{Pass, SentUnit, ids_of};
use crate::domain::finding::{Finding, FindingId, Place, Status};
use crate::domain::log_text::section;
use crate::domain::scope::Scope;

/// The units the user edited while the pass was looking at them: their
/// code is no longer what was sent to the reviewer, so what the reviewer
/// said about them is stale.
///
/// Example: `cancel` was sent to the reviewer at fingerprint 0e41bd1, and
/// while the call ran the user saved a change to it. `cancel` is a changed
/// unit: the findings the reviewer raised on it are dropped, and it is not
/// marked reviewed, so the next run sends it again.
struct ChangedUnits(HashSet<(PathBuf, String)>);

impl ChangedUnits {
    /// Whether the unit `name` in `file` changed.
    fn contains(&self, file: &Path, name: &str) -> bool {
        self.0.contains(&(file.to_path_buf(), name.to_string()))
    }

    /// Whether the place is in a changed unit.
    fn holds(&self, place: &Place) -> bool {
        place
            .unit
            .as_deref()
            .is_some_and(|u| self.contains(&place.file, u))
    }

    /// The sent units that did not change.
    fn not_changed<'a>(&self, sent: &'a [SentUnit]) -> Vec<&'a SentUnit> {
        sent.iter()
            .filter(|u| !self.contains(&u.file, &u.unit))
            .collect()
    }
}

/// What looking for findings the reviewer did not raise again settled.
enum NotFoundAgain {
    /// Every unit was reviewed: these findings were not raised again in
    /// their changed unit, so they were fixed and are resolved.
    Resolved(Vec<FindingId>),
    /// Some unit was not reviewed (a spending limit, or a call that
    /// failed): a finding not raised again may just not have been looked
    /// at, so nothing is resolved this way.
    NotChecked,
}

impl Pass<'_, '_> {
    /// Remembers what the review settled:
    ///
    /// 1. findings the reviewer said no longer hold are resolved;
    /// 2. new findings in units the user edited during the review are
    ///    dropped (they are about code that is no longer there);
    /// 3. only when every unit was reviewed: a stored finding in a unit
    ///    whose code changed, which the reviewer neither raised again nor
    ///    said still holds, was fixed, and is resolved;
    /// 4. all of it is logged, with the findings `sweep_gone` found gone and
    ///    those `sweep_put_back` resolved.
    ///
    /// Example: `cancel` had an open finding "returns None". You changed
    /// `cancel` and ran scry; the reviewer looked at the new `cancel` and
    /// did not raise it again. Every unit was reviewed, so it is resolved.
    pub(super) fn remember(
        &mut self,
        review: &ReviewResults,
        gone: &[FindingId],
        put_back: &[FindingId],
    ) {
        let resolved = self.resolve_what_no_longer_holds(&review.no_longer_hold);
        let changed = self.drop_new_findings_in_changed_units(&review.answered);
        let not_found_again = if review.every_unit_was_reviewed() {
            let still_as_sent = changed.not_changed(&review.answered);
            NotFoundAgain::Resolved(self.resolve_not_found_again(&still_as_sent, review))
        } else {
            NotFoundAgain::NotChecked
        };
        self.log(&remember_text(
            &resolved,
            gone,
            put_back,
            &changed,
            &not_found_again,
        ));
    }

    /// Resolves the open and unverified findings the reviewer said no
    /// longer hold (given by id); returns their ids.
    fn resolve_what_no_longer_holds(&mut self, no_longer_hold: &HashSet<String>) -> Vec<FindingId> {
        let ids: Vec<FindingId> = self
            .working_copy
            .findings
            .iter()
            .filter(|f| no_longer_hold.contains(&f.id.0) && is_open_or_unverified(f))
            .map(|f| f.id.clone())
            .collect();
        self.close_for_changed_code(&ids, Status::Resolved);
        ids
    }

    /// Resolves the findings that were fixed: see `was_fixed`. The units
    /// given are the ones the reviewer saw, unchanged since. Returns their
    /// ids.
    fn resolve_not_found_again(
        &mut self,
        still_as_sent: &[&SentUnit],
        review: &ReviewResults,
    ) -> Vec<FindingId> {
        let sent_at: HashMap<(&Path, &str), Fingerprint> = still_as_sent
            .iter()
            .map(|u| ((u.file.as_path(), u.unit.as_str()), u.fingerprint))
            .collect();
        let stored_before = ids_of(&self.stored_before);
        let ids: Vec<FindingId> = self
            .working_copy
            .findings
            .iter()
            .filter(|f| self.was_fixed(f, &sent_at, &stored_before, review))
            .map(|f| f.id.clone())
            .collect();
        self.close_for_changed_code(&ids, Status::Resolved);
        ids
    }

    /// Whether a finding was fixed: it was stored before this run and is
    /// still open or unverified; the reviewer said nothing about it (did
    /// not raise it again, nor say it holds or no longer holds); and its
    /// unit was sent to the reviewer with different code than the finding
    /// was made on.
    fn was_fixed(
        &self,
        f: &Finding,
        sent_at: &HashMap<(&Path, &str), Fingerprint>,
        stored_before: &HashSet<FindingId>,
        review: &ReviewResults,
    ) -> bool {
        let reviewer_said_nothing = !self.merged.contains(&f.id)
            && !review.no_longer_hold.contains(&f.id.0)
            && !review.still_hold.contains(&f.id.0);
        let its_unit_changed = f
            .place
            .unit
            .as_deref()
            .and_then(|u| sent_at.get(&(f.place.file.as_path(), u)))
            .zip(f.place.fingerprint)
            .is_some_and(|(sent, made_on)| *sent != made_on);
        stored_before.contains(&f.id)
            && is_open_or_unverified(f)
            && reviewer_said_nothing
            && its_unit_changed
    }

    /// Marks the units the reviewer answered for as reviewed, at the
    /// fingerprint they were sent at, so the next run does not send them
    /// again until they change. A unit the user edited while the paid
    /// checks ran is left out (and its new findings dropped).
    pub(super) fn mark_reviewed(&mut self, answered: &[SentUnit]) {
        let changed = self.drop_new_findings_in_changed_units(answered);
        let reviewed = changed.not_changed(answered);
        for unit in &reviewed {
            self.mark_one_reviewed(unit);
        }
        self.log(&reviewed_text(&changed, &reviewed));
    }

    /// Records the unit as reviewed at the fingerprint it was sent at, and
    /// no longer held back by a spending limit.
    fn mark_one_reviewed(&mut self, unit: &SentUnit) {
        self.working_copy
            .reviewed
            .entry(unit.file.clone())
            .or_default()
            .insert(unit.fingerprint.to_string());
        if let Some(names) = self.working_copy.capped.get_mut(&unit.file) {
            names.remove(&unit.unit);
            if names.is_empty() {
                self.working_copy.capped.remove(&unit.file);
            }
        }
    }

    /// Drops this run's new findings in the units the user edited since
    /// they were sent; returns those units.
    fn drop_new_findings_in_changed_units(&mut self, sent: &[SentUnit]) -> ChangedUnits {
        let changed = self.units_changed_since_sent(sent);
        let dropped: HashSet<FindingId> = self
            .working_copy
            .findings
            .iter()
            .filter(|f| self.new.contains(&f.id) && changed.holds(&f.place))
            .map(|f| f.id.clone())
            .collect();
        self.working_copy
            .findings
            .retain(|f| !dropped.contains(&f.id));
        self.new.retain(|id| !dropped.contains(id));
        changed
    }

    /// Before the review: open, unverified and withheld findings whose unit
    /// is no longer in its file (or whose file was deleted) are gone.
    /// Returns their ids.
    pub(super) fn sweep_gone(&mut self, scope: &Scope) -> Vec<FindingId> {
        let ids: Vec<FindingId> = scope
            .files
            .iter()
            .flat_map(|sf| {
                self.working_copy.findings.iter().filter(move |f| {
                    let unit_left = sf.deleted
                        || f.place
                            .unit
                            .as_ref()
                            .is_some_and(|u| !sf.units_now.contains(u));
                    f.place.file == sf.file
                        && unit_left
                        && matches!(
                            f.status,
                            Status::Open | Status::Unverified | Status::Withheld
                        )
                })
            })
            .map(|f| f.id.clone())
            .collect();
        self.close_for_changed_code(&ids, Status::Gone);
        ids
    }

    /// Before the review: an open or unverified finding whose unit is now
    /// the same as at the base, at a fingerprint other than the finding's,
    /// is resolved without a call (a finding stored with no fingerprint is
    /// left alone). Returns their ids.
    ///
    /// Example: you saved a change to `cancel`, a pass raised a finding on
    /// it, and you undid the change. `cancel` is what `HEAD` has again, so
    /// no pass would review it; the finding is about code that is no longer
    /// there, and is resolved. Had you committed the change instead, the
    /// fingerprint would still be the finding's, and it would stay open.
    pub(super) fn sweep_put_back(&mut self, scope: &Scope) -> Vec<FindingId> {
        let put_back = |f: &Finding| {
            scope.files.iter().any(|sf| {
                sf.file == f.place.file
                    && f.place.unit.as_ref().is_some_and(|name| {
                        sf.same
                            .iter()
                            .any(|(same, at)| same == name && reviewed_at_other(f, *at))
                    })
            })
        };
        let ids: Vec<FindingId> = self
            .working_copy
            .findings
            .iter()
            .filter(|f| is_open_or_unverified(f) && put_back(f))
            .map(|f| f.id.clone())
            .collect();
        self.close_for_changed_code(&ids, Status::Resolved);
        ids
    }

    /// The sent units whose code is not what was sent: their fingerprint
    /// changed, or they are no longer in their file.
    fn units_changed_since_sent(&self, sent: &[SentUnit]) -> ChangedUnits {
        let mut now: BTreeMap<&Path, Vec<Unit>> = BTreeMap::new();
        for unit in sent {
            now.entry(unit.file.as_path())
                .or_insert_with(|| self.deps.view.units(&unit.file).unwrap_or_default());
        }
        ChangedUnits(
            sent.iter()
                .filter(|sent_unit| {
                    !now[sent_unit.file.as_path()]
                        .iter()
                        .any(|u| u.name == sent_unit.unit && u.fingerprint == sent_unit.fingerprint)
                })
                .map(|u| (u.file.clone(), u.unit.clone()))
                .collect(),
        )
    }

    /// Closes each finding in `ids` (resolved or gone, dated now) because
    /// its code changed, and forgets that the code it was made on was
    /// reviewed: if the unit comes back to that code, it is sent again
    /// rather than counted as reviewed with nothing to say.
    ///
    /// Example: a finding on `cancel` at fingerprint 0e41bd1 is resolved
    /// because you put `cancel` back as the base has it. You then redo the
    /// change: `cancel` is at 0e41bd1 again, and the next pass sends it, so
    /// the bug shows again. A unit the reviewer answered for in this pass is
    /// marked reviewed afterwards (`mark_reviewed`), so a finding the
    /// reviewer itself said no longer holds is not sent again for nothing.
    fn close_for_changed_code(&mut self, ids: &[FindingId], status: Status) {
        self.forget_code_reviewed_for(ids);
        self.set_status_of(ids, status);
    }

    /// Takes the fingerprint each finding in `ids` was made on out of its
    /// file's reviewed ones.
    fn forget_code_reviewed_for(&mut self, ids: &[FindingId]) {
        let made_on: Vec<(PathBuf, String)> = self
            .working_copy
            .findings
            .iter()
            .filter(|f| ids.contains(&f.id))
            .filter_map(|f| Some((f.place.file.clone(), f.place.fingerprint?.to_string())))
            .collect();
        for (file, print) in made_on {
            if let Some(prints) = self.working_copy.reviewed.get_mut(&file) {
                prints.remove(&print);
            }
        }
        self.working_copy
            .reviewed
            .retain(|_, prints| !prints.is_empty());
    }

    /// Sets the status of each finding in `ids`, dated now.
    fn set_status_of(&mut self, ids: &[FindingId], status: Status) {
        let now = self.now;
        for f in self
            .working_copy
            .findings
            .iter_mut()
            .filter(|f| ids.contains(&f.id))
        {
            f.set_status(status, now);
        }
    }
}

/// Whether the finding was raised on its unit at a fingerprint other than
/// `at`. A finding stored with no fingerprint cannot tell, and says no.
fn reviewed_at_other(f: &Finding, at: Fingerprint) -> bool {
    f.place.fingerprint.is_some_and(|reviewed| reviewed != at)
}

fn is_open_or_unverified(f: &Finding) -> bool {
    matches!(f.status, Status::Open | Status::Unverified)
}

// The run log's text for what the pass remembers.

/// The Remember section: resolved, gone, dropped, and what not being
/// found again settled.
fn remember_text(
    resolved: &[FindingId],
    gone: &[FindingId],
    put_back: &[FindingId],
    changed: &ChangedUnits,
    not_found_again: &NotFoundAgain,
) -> String {
    let mut text = section("Remember");
    for id in resolved {
        text.push_str(&format!(
            "- {} resolved: the reviewer said it no longer holds\n",
            id.0
        ));
    }
    for id in gone {
        text.push_str(&format!(
            "- {} gone: its unit or file is no longer there\n",
            id.0
        ));
    }
    for id in put_back {
        text.push_str(&format!(
            "- {} resolved: its unit is back to the code at the base\n",
            id.0
        ));
    }
    for (file, name) in &changed.0 {
        text.push_str(&format!(
            "- `{}` `{}` changed while the pass ran: its new findings are dropped, it is not reviewed\n",
            forward_slashes(file),
            name
        ));
    }
    match not_found_again {
        NotFoundAgain::Resolved(ids) => {
            for id in ids {
                text.push_str(&format!(
                    "- {} resolved: its unit changed and the reviewer did not raise it again\n",
                    id.0
                ));
            }
        }
        NotFoundAgain::NotChecked => text.push_str(
            "- the pass was not whole (a cap or a failure): nothing is resolved for not being found again\n",
        ),
    }
    if text.trim_end().ends_with("## Remember") {
        text.push_str("- nothing resolved or gone\n");
    }
    text
}

/// The Reviewed section: the units the user edited while the checks ran,
/// then those now reviewed.
fn reviewed_text(changed: &ChangedUnits, reviewed: &[&SentUnit]) -> String {
    let mut text = section("Reviewed");
    for (file, name) in &changed.0 {
        text.push_str(&format!(
            "- `{}` `{}` changed while the checks ran: not reviewed\n",
            forward_slashes(file),
            name
        ));
    }
    for unit in reviewed {
        text.push_str(&format!(
            "- `{}` `{}` reviewed at {}\n",
            forward_slashes(&unit.file),
            unit.unit,
            unit.fingerprint
        ));
    }
    if text.trim_end().ends_with("## Reviewed") {
        text.push_str("- no unit newly reviewed\n");
    }
    text
}
