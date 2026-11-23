//! The command line. Each command reads its arguments, calls one use case and prints what comes
//! back. The exit code is 0 when the command did its work, and 2 when spellbook could not run or
//! stopped to ask for an input.

mod arguments;
mod console;
mod doctor;
mod format;
mod holes;
mod list;
mod lsp;
mod new;
mod run_cli;
mod show;
mod trust;

pub use doctor::{DoctorRow, doctor_rows};
pub use run_cli::{main, main_with};
