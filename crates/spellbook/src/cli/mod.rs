//! The command line. Each command reads its arguments, calls one use case and prints what comes
//! back. The exit code is 0 when the command did its work, and 2 when spellbook could not run or
//! stopped to ask for an input.

// spellbook:region cli-modules (from crates/spellbook/src/cli/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
mod arguments;
mod check;
mod console;
mod doctor;
mod file_arguments;
mod fix;
mod format;
mod holes;
mod init;
mod list;
mod lsp;
mod new;
mod rules;
mod run_cli;
mod show;
mod sync;
mod trust;
// spellbook:end cli-modules

pub use doctor::{DoctorRow, doctor_rows};
pub use run_cli::{main, main_with};
