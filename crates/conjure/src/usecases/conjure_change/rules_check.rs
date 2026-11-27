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
pub(super) fn rules_broken_by(files: &[ChangedFile], deps: &ChangeDeps) -> Checked {
    let after: Vec<(PathBuf, String)> = files
        .iter()
        .map(|file| (file.path.clone(), file.after.clone()))
        .collect();
    let found_after = match checked(deps, &after) {
        Reading::Findings(found) if any_error(&found) => found,
        Reading::TimedOut => return Checked::timed_out(),
        _ => return Checked::broke(Vec::new()),
    };
    let before: Vec<(PathBuf, String)> = files
        .iter()
        .filter_map(|file| Some((file.path.clone(), file.before.clone()?)))
        .collect();
    match checked(deps, &before) {
        Reading::Findings(found_before) => {
            Checked::broke(rules_broken(&found_before, &found_after))
        }
        Reading::TimedOut => Checked::timed_out(),
        Reading::Nothing => Checked::broke(Vec::new()),
    }
}

/// What checking a change's files gave.
pub(super) struct Checked {
    pub broken: Vec<RuleFinding>,
    /// The check did not answer in time, so nothing is known about the rules.
    pub timed_out: bool,
}

impl Checked {
    fn broke(broken: Vec<RuleFinding>) -> Checked {
        Checked {
            broken,
            timed_out: false,
        }
    }

    fn timed_out() -> Checked {
        Checked {
            broken: Vec::new(),
            timed_out: true,
        }
    }
}

enum Reading {
    Findings(Vec<RuleFinding>),
    TimedOut,
    /// Could not be checked for another reason (no spellbook, no rules): silent.
    Nothing,
}

/// The findings in the texts; why not when they could not be checked.
fn checked(deps: &ChangeDeps, texts: &[(PathBuf, String)]) -> Reading {
    match deps.checker.findings_in(texts) {
        RulesRead::Checked(findings) => Reading::Findings(findings),
        RulesRead::TimedOut => Reading::TimedOut,
        RulesRead::NotChecked(_) => Reading::Nothing,
    }
}
