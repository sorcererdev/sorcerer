//! Conjuring one change: find the unit, read the situation, ask the writer once, check what it
//! wrote. Nothing is written to disk; the caller lands the change (the editor's server through
//! `workspace/applyEdit`; the command line only prints it).

// spellbook:region usecase-parts (from crates/conjure/src/usecases/conjure_change/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
mod check;
mod find_site;
mod request_and_result;
mod rules_check;
mod run_conjure;
mod situation;
mod write;
// spellbook:end usecase-parts

use find_site::Site;
pub use request_and_result::{
    ChangeDeps, ChangeRefused, ChangeRequest, ChangedFile, ConjuredChange,
};
pub use run_conjure::conjure_change;
