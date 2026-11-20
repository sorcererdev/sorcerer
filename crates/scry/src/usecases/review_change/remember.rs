//! What the pass remembers: findings the reviewer resolved, those whose
//! unit is gone, those not found again, new findings in units that changed
//! while the pass ran (dropped), and the units marked reviewed.

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
        let (answered, no_longer, held) =
            (&asked.answered, &asked.no_longer_hold, &asked.still_hold);
        // resolved by the review call's answer
        let now = self.now;
        let mut remember = section("Remember");
        for f in &mut self.working_copy.findings {
            if no_longer.contains(&f.id.0) && matches!(f.status, Status::Open | Status::Waiting) {
                f.set_status(Status::Resolved, now);
                remember.push_str(&format!(
                    "- {} resolved: the reviewer said it no longer holds\n",
                    f.id.0
                ));
            }
        }
        for id in gone {
            remember.push_str(&format!(
                "- {} gone: its unit or file is no longer there\n",
                id.0
            ));
        }

        // a unit whose text changed while the pass ran keeps no new findings
        // and is not reviewed
        let moved = self.drop_moved(answered);
        for (file, name) in &moved {
            remember.push_str(&format!(
                "- `{}` `{}` changed while the pass ran: its new findings are dropped, it is not reviewed\n",
                file.display(),
                name
            ));
        }
        let kept: Vec<&SentUnit> = answered
            .iter()
            .filter(|u| !moved.contains(&(u.file.clone(), u.name.clone())))
            .collect();

        // not found again: an open or waiting finding whose unit this pass
        // looked at with a new fingerprint, that the review call neither
        // re-raised, said still holds, nor resolved, is resolved; only when
        // the pass was whole
        if whole {
            let at: HashMap<(&Path, &str), Fingerprint> = kept
                .iter()
                .map(|u| ((u.file.as_path(), u.name.as_str()), u.fingerprint))
                .collect();
            for f in &mut self.working_copy.findings {
                let sent_at = f
                    .place
                    .unit
                    .as_deref()
                    .and_then(|u| at.get(&(f.place.file.as_path(), u)));
                let not_found = base_ids.contains(&f.id)
                    && matches!(f.status, Status::Open | Status::Waiting)
                    && !self.merged.contains(&f.id)
                    && !no_longer.contains(&f.id.0)
                    && !held.contains(&f.id.0)
                    && matches!((sent_at, f.place.fingerprint), (Some(now_fingerprint), Some(was)) if *now_fingerprint != was);
                if not_found {
                    f.set_status(Status::Resolved, now);
                    remember.push_str(&format!(
                        "- {} resolved: its unit changed and the reviewer did not raise it again\n",
                        f.id.0
                    ));
                }
            }
        } else {
            remember.push_str(
                "- the pass was not whole (a cap or a failure): nothing is resolved for not being found again\n",
            );
        }
        if remember.trim_end().ends_with("## Remember") {
            remember.push_str("- nothing resolved or gone\n");
        }
        self.log(&remember);
    }

    /// The units answered and not moved since, marked reviewed at the
    /// fingerprint sent (and no longer capped); the Reviewed section.
    pub(super) fn mark_reviewed(&mut self, answered: &[SentUnit]) {
        // checked again: a unit may have changed while the checks ran
        let moved = self.drop_moved(answered);
        let mut written = section("Reviewed");
        for (file, name) in &moved {
            written.push_str(&format!(
                "- `{}` `{}` changed while the checks ran: not reviewed\n",
                file.display(),
                name
            ));
        }
        for SentUnit {
            file,
            name,
            fingerprint,
            ..
        } in answered
            .iter()
            .filter(|u| !moved.contains(&(u.file.clone(), u.name.clone())))
        {
            written.push_str(&format!(
                "- `{}` `{name}` reviewed at {fingerprint}\n",
                file.display()
            ));
            self.working_copy
                .reviewed
                .entry(file.clone())
                .or_default()
                .insert(fingerprint.to_string());
            if let Some(names) = self.working_copy.capped.get_mut(file) {
                names.remove(name);
                if names.is_empty() {
                    self.working_copy.capped.remove(file);
                }
            }
        }

        if written.trim_end().ends_with("## Reviewed") {
            written.push_str("- no unit newly reviewed\n");
        }
        self.log(&written);
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

    /// Open, waiting and withheld findings of a file in scope whose unit is
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
                    && matches!(f.status, Status::Open | Status::Waiting | Status::Withheld)
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

fn in_moved(moved: &HashSet<(PathBuf, String)>, pl: &Place) -> bool {
    pl.unit
        .as_ref()
        .is_some_and(|u| moved.contains(&(pl.file.clone(), u.clone())))
}
