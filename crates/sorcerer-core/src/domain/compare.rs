//! Comparing a file's units before and after a change.

use std::collections::HashMap;

use crate::domain::unit::Unit;

/// What a change did to one unit (a function, a class, …) of a file.
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

/// Pairs a file's units from before a change with its units after, and says of each whether it
/// was added, changed, removed or stayed the same.
///
/// Units pair by name. When a file has several units with one name, they pair in turn: the first
/// `Order` before with the first `Order` after, the second with the second. A pair with the same
/// fingerprint is `Same`, otherwise `Changed`. A unit after with no partner is `Added`; one
/// before with no partner is `Removed`.
///
/// The list follows the file's order after the change, then the removed units in their order
/// before.
///
/// Example: before `[load, Order, save]` and after `[Order (edited), save, print]` gives
/// `[Changed Order, Same save, Added print, Removed load]`.
pub fn compare_units(before: &[Unit], after: &[Unit]) -> Vec<UnitChange> {
    let mut unpaired_before: HashMap<NameAndTurn, &Unit> = with_name_and_turn(before).collect();
    let changes_after: Vec<UnitChange> = with_name_and_turn(after)
        .map(|(key, unit)| change_to(unit, unpaired_before.remove(&key)))
        .collect();
    let removed = with_name_and_turn(before)
        .filter(|(key, _)| unpaired_before.contains_key(key))
        .map(|(_, unit)| UnitChange::Removed(unit.clone()));
    changes_after.into_iter().chain(removed).collect()
}

/// Which unit of its name a unit is: the second `Order` in a file is `("Order", 2)`.
#[derive(PartialEq, Eq, Hash)]
struct NameAndTurn<'a> {
    name: &'a str,
    turn: usize,
}

/// Each unit with its name and turn, in file order.
fn with_name_and_turn(units: &[Unit]) -> impl Iterator<Item = (NameAndTurn<'_>, &Unit)> {
    let mut seen_so_far: HashMap<&str, usize> = HashMap::new();
    units.iter().map(move |unit| {
        let turn = seen_so_far.entry(unit.name.as_str()).or_default();
        *turn += 1;
        let key = NameAndTurn {
            name: unit.name.as_str(),
            turn: *turn,
        };
        (key, unit)
    })
}

/// What happened to a unit, given its partner from before the change (if it had one).
fn change_to(after: &Unit, before: Option<&Unit>) -> UnitChange {
    match before {
        None => UnitChange::Added(after.clone()),
        Some(before) if before.fingerprint == after.fingerprint => UnitChange::Same(after.clone()),
        Some(before) => UnitChange::Changed {
            before: before.clone(),
            after: after.clone(),
        },
    }
}
