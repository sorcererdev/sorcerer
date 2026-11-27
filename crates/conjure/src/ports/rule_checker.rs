//! The rule-checker seam: the project's checked rules (spellbook's) run over texts that are not
//! on disk yet, so a change is checked before it lands.

use std::path::PathBuf;

use crate::domain::spellbook_findings::RulesRead;

pub trait RuleChecker: Send + Sync {
    /// The findings of the project's rules in these texts, each read as the file at its path
    /// (relative to the root); the files on disk are not changed. No texts is no findings.
    fn findings_in(&self, texts: &[(PathBuf, String)]) -> RulesRead;

    /// What a person reads about how changes are checked (for `conjure doctor`).
    ///
    /// Example: `/usr/local/bin/spellbook: changes are checked against
    /// .sorcerer/spellbook/rules.toml`.
    fn describe(&self) -> String;
}
