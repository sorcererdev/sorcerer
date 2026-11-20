//! What the pass remembers. `remember`, after the findings are judged:
//! 1. resolves the findings the reviewer said no longer hold
//!    (`resolve_what_no_longer_holds`);
//! 2. names those whose unit is gone (`sweep_gone` found them before the
//!    calls);
//! 3. drops the new findings of units that changed while the pass ran
//!    (`drop_moved`);
//! 4. only when the pass was whole, resolves those not found again
//!    (`resolve_not_found_again`).
//!
//! `mark_reviewed`, after the rounds, marks each answered unit that has not
//! moved since reviewed at the fingerprint it was sent at
//! (`mark_one_reviewed`).
//!
//! The run log's text is built last in the file.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::unit::Unit;

use super::ask::ReviewResults;
use super::{Pass, SentUnit};
use crate::domain::finding::{FindingId, Place, Status};
use crate::domain::log_text::section;
use crate::domain::scope::Scope;

impl Pass<'_, '_> {
    /// The Remember section: resolved by the reviewer's answer, gone, moved
    /// while the pass ran, and (only when the pass was WHOLE) not found again.
    pub(super) fn remember(
        &mut self,
        asked: &ReviewResults,
        gone: &[FindingId],
        base_ids: &HashSet<FindingId>,
        whole: bool,
    ) {
        let resolved = self.resolve_what_no_longer_holds(&asked.no_longer_hold);
        let moved = self.drop_moved(&asked.answered);
        let not_found_again = if whole {
            let kept = not_moved(&asked.answered, &moved);
            Some(self.resolve_not_found_again(&kept, asked, base_ids))
        } else {
            None
        };
        self.log(&remember_text(&resolved, gone, &moved, not_found_again));
    }

    /// Resolves the open and unverified findings a review call said no longer
    /// hold (NO_LONGER_HOLD, by id); their ids.
    fn resolve_what_no_longer_holds(&mut self, no_longer_hold: &HashSet<String>) -> Vec<FindingId> {
        let now = self.now;
        let mut resolved = Vec::new();
        for f in &mut self.working_copy.findings {
            if no_longer_hold.contains(&f.id.0)
                && matches!(f.status, Status::Open | Status::Unverified)
            {
                f.set_status(Status::Resolved, now);
                resolved.push(f.id.clone());
            }
        }
        resolved
    }

    /// Not found again: an open or unverified finding whose unit this pass
    /// looked at (one of KEPT) with a new fingerprint, that the review call
    /// neither re-raised, said still holds, nor resolved, is resolved. Only
    /// when the pass was whole: the caller does not ask otherwise. Their ids.
    fn resolve_not_found_again(
        &mut self,
        kept: &[&SentUnit],
        asked: &ReviewResults,
        base_ids: &HashSet<FindingId>,
    ) -> Vec<FindingId> {
        let at: HashMap<(&Path, &str), Fingerprint> = kept
            .iter()
            .map(|u| ((u.file.as_path(), u.name.as_str()), u.fingerprint))
            .collect();
        let now = self.now;
        let mut resolved = Vec::new();
        for f in &mut self.working_copy.findings {
            let sent_at = f
                .place
                .unit
                .as_deref()
                .and_then(|u| at.get(&(f.place.file.as_path(), u)));
            let not_found = base_ids.contains(&f.id)
                && matches!(f.status, Status::Open | Status::Unverified)
                && !self.merged.contains(&f.id)
                && !asked.no_longer_hold.contains(&f.id.0)
                && !asked.still_hold.contains(&f.id.0)
                && matches!((sent_at, f.place.fingerprint), (Some(now_fingerprint), Some(was)) if *now_fingerprint != was);
            if not_found {
                f.set_status(Status::Resolved, now);
                resolved.push(f.id.clone());
            }
        }
        resolved
    }

    /// The units answered and not moved since, marked reviewed at the
    /// fingerprint sent (and no longer capped); the Reviewed section.
    pub(super) fn mark_reviewed(&mut self, answered: &[SentUnit]) {
        // checked again: a unit may have changed while the checks ran
        let moved = self.drop_moved(answered);
        let reviewed = not_moved(answered, &moved);
        for unit in &reviewed {
            self.mark_one_reviewed(unit);
        }
        self.log(&reviewed_text(&moved, &reviewed));
    }

    /// UNIT reviewed at the fingerprint it was sent at, and no longer capped.
    fn mark_one_reviewed(&mut self, unit: &SentUnit) {
        self.working_copy
            .reviewed
            .entry(unit.file.clone())
            .or_default()
            .insert(unit.fingerprint.to_string());
        if let Some(names) = self.working_copy.capped.get_mut(&unit.file) {
            names.remove(&unit.name);
            if names.is_empty() {
                self.working_copy.capped.remove(&unit.file);
            }
        }
    }

    /// Drops this pass's new findings in units whose text changed since
    /// they were sent; returns those units.
    pub(super) fn drop_moved(&mut self, answered: &[SentUnit]) -> HashSet<(PathBuf, String)> {
        let moved = self.moved(answered);
        let new_ids: HashSet<FindingId> = self.new.iter().cloned().collect();
        let dropped: HashSet<FindingId> = self
            .working_copy
            .findings
            .iter()
            .filter(|f| new_ids.contains(&f.id) && in_moved(&moved, &f.place))
            .map(|f| f.id.clone())
            .collect();
        self.working_copy
            .findings
            .retain(|f| !dropped.contains(&f.id));
        self.new.retain(|i| !dropped.contains(i));
        moved
    }

    /// Open, unverified and withheld findings of a file in scope whose unit is
    /// no longer in it (or whose file was deleted) are gone.
    pub(super) fn sweep_gone(&mut self, scope: &Scope) -> Vec<FindingId> {
        let now = self.now;
        let mut gone = Vec::new();
        for sf in &scope.files {
            for f in &mut self.working_copy.findings {
                let left = sf.deleted
                    || f.place
                        .unit
                        .as_ref()
                        .is_some_and(|u| !sf.units_now.contains(u));
                if f.place.file == sf.file
                    && left
                    && matches!(
                        f.status,
                        Status::Open | Status::Unverified | Status::Withheld
                    )
                {
                    f.set_status(Status::Gone, now);
                    gone.push(f.id.clone());
                }
            }
        }
        gone
    }

    /// The sent units whose fingerprint is not what was sent (or that are
    /// no longer in their file).
    fn moved(&self, sent: &[SentUnit]) -> HashSet<(PathBuf, String)> {
        let mut now: BTreeMap<&Path, Vec<Unit>> = BTreeMap::new();
        for unit in sent {
            now.entry(unit.file.as_path())
                .or_insert_with(|| self.deps.view.units(&unit.file).unwrap_or_default());
        }
        sent.iter()
            .filter(|sent_unit| {
                !now[sent_unit.file.as_path()]
                    .iter()
                    .any(|u| u.name == sent_unit.name && u.fingerprint == sent_unit.fingerprint)
            })
            .map(|u| (u.file.clone(), u.name.clone()))
            .collect()
    }
}

/// The units of ANSWERED that are not in MOVED.
fn not_moved<'a>(
    answered: &'a [SentUnit],
    moved: &HashSet<(PathBuf, String)>,
) -> Vec<&'a SentUnit> {
    answered
        .iter()
        .filter(|u| !moved.contains(&(u.file.clone(), u.name.clone())))
        .collect()
}

fn in_moved(moved: &HashSet<(PathBuf, String)>, pl: &Place) -> bool {
    pl.unit
        .as_ref()
        .is_some_and(|u| moved.contains(&(pl.file.clone(), u.clone())))
}

// The run log's text for what the pass remembers.

/// The Remember section. NOT_FOUND_AGAIN is None when the pass was not
/// whole, so nothing was resolved for not being found again.
fn remember_text(
    resolved: &[FindingId],
    gone: &[FindingId],
    moved: &HashSet<(PathBuf, String)>,
    not_found_again: Option<Vec<FindingId>>,
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
    for (file, name) in moved {
        text.push_str(&format!(
            "- `{}` `{}` changed while the pass ran: its new findings are dropped, it is not reviewed\n",
            file.display(),
            name
        ));
    }
    match not_found_again {
        Some(ids) => {
            for id in ids {
                text.push_str(&format!(
                    "- {} resolved: its unit changed and the reviewer did not raise it again\n",
                    id.0
                ));
            }
        }
        None => text.push_str(
            "- the pass was not whole (a cap or a failure): nothing is resolved for not being found again\n",
        ),
    }
    if text.trim_end().ends_with("## Remember") {
        text.push_str("- nothing resolved or gone\n");
    }
    text
}

/// The Reviewed section: the units that MOVED while the checks ran, then
/// those REVIEWED.
fn reviewed_text(moved: &HashSet<(PathBuf, String)>, reviewed: &[&SentUnit]) -> String {
    let mut text = section("Reviewed");
    for (file, name) in moved {
        text.push_str(&format!(
            "- `{}` `{}` changed while the checks ran: not reviewed\n",
            file.display(),
            name
        ));
    }
    for unit in reviewed {
        text.push_str(&format!(
            "- `{}` `{}` reviewed at {}\n",
            unit.file.display(),
            unit.name,
            unit.fingerprint
        ));
    }
    if text.trim_end().ends_with("## Reviewed") {
        text.push_str("- no unit newly reviewed\n");
    }
    text
}
