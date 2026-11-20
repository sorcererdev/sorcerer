//! Comparing a file's units before and after a change.

use std::collections::HashMap;

use crate::domain::unit::Unit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitChange {
    Added(Unit),
    Changed { before: Unit, after: Unit },
    Removed(Unit),
    Same(Unit),
}

impl UnitChange {
    /// The unit as it is now, when it was added or changed.
    pub fn unit_now(self) -> Option<Unit> {
        match self {
            UnitChange::Added(unit) | UnitChange::Changed { after: unit, .. } => Some(unit),
            UnitChange::Removed(_) | UnitChange::Same(_) => None,
        }
    }
}

/// Units pair by name and by which of that name they are (the second
/// `Order` with the second `Order`); a pair with equal fingerprints is
/// `Same`, else `Changed`. Unpaired units after are `Added`, before are
/// `Removed`. The order is AFTER's, then the removed in BEFORE's order.
pub fn compare_units(before: &[Unit], after: &[Unit]) -> Vec<UnitChange> {
    let keyed = |us: &[Unit]| -> Vec<(String, usize)> {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        us.iter()
            .map(|u| {
                let n = seen.entry(u.name.as_str()).or_default();
                *n += 1;
                (u.name.clone(), *n)
            })
            .collect()
    };
    let before_keys = keyed(before);
    let mut by_key: HashMap<(String, usize), &Unit> =
        before_keys.iter().cloned().zip(before).collect();
    let mut out: Vec<UnitChange> = keyed(after)
        .into_iter()
        .zip(after)
        .map(|(k, a)| match by_key.remove(&k) {
            Some(b) if b.fingerprint == a.fingerprint => UnitChange::Same(a.clone()),
            Some(b) => UnitChange::Changed {
                before: b.clone(),
                after: a.clone(),
            },
            None => UnitChange::Added(a.clone()),
        })
        .collect();
    out.extend(
        before_keys
            .into_iter()
            .zip(before)
            .filter(|(k, _)| by_key.contains_key(k))
            .map(|(_, b)| UnitChange::Removed(b.clone())),
    );
    out
}
