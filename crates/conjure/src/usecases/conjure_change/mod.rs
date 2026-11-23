//! Conjuring one change: find the unit, read the situation, ask the writer once, check what it
//! wrote. Nothing is written to disk; the caller lands the change (the editor's server through
//! `workspace/applyEdit`; the command line only prints it).

mod check;
mod find_site;
mod request_and_result;
mod run_conjure;
mod situation;
mod write;

use find_site::Site;
pub use request_and_result::{
    ChangeDeps, ChangeRefused, ChangeRequest, ChangedFile, ConjuredChange,
};
pub use run_conjure::conjure_change;
