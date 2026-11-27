//! Checking a change against the project's checked rules (spellbook's) before it lands.

use std::path::PathBuf;

use super::{ChangeDeps, ChangedFile};
use crate::domain::spellbook_findings::{RuleFinding, RulesRead, any_error, rules_broken};

/// The rules a change's files break that their texts before the change did not: the errors in
/// the new texts less those already there. A change the rules cannot be checked on (no
/// spellbook, no rules, spellbook stopped) breaks none; spellbook is not asked about the old
/// texts when the new ones have no error.
///
/// Example: a fill that writes `std::fs::write(…)` in a command where `commands-use-ports`
/// forbids it, in place of a placeholder, breaks that rule once.
pub(super) fn rules_broken_by(files: &[ChangedFile], deps: &ChangeDeps) -> Vec<RuleFinding> {
    let after: Vec<(PathBuf, String)> = files
        .iter()
        .map(|file| (file.path.clone(), file.after.clone()))
        .collect();
    let Some(found_after) = checked(deps, &after).filter(|found| any_error(found)) else {
        return Vec::new();
    };
    let before: Vec<(PathBuf, String)> = files
        .iter()
        .filter_map(|file| Some((file.path.clone(), file.before.clone()?)))
        .collect();
    match checked(deps, &before) {
        Some(found_before) => rules_broken(&found_before, &found_after),
        None => Vec::new(),
    }
}

/// The findings in the texts; none when they could not be checked.
fn checked(deps: &ChangeDeps, texts: &[(PathBuf, String)]) -> Option<Vec<RuleFinding>> {
    match deps.checker.findings_in(texts) {
        RulesRead::Checked(findings) => Some(findings),
        RulesRead::NotChecked(_) => None,
    }
}
