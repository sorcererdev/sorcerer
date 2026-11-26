//! The command line. `conjure FILE:LINE[-END]` conjures one change at that place and prints it
//! (the goal, each file's diff and whether it parses); it lands nothing, since only an editor
//! can land a change. `--finding ID` conjures for a scry finding, `--try-harder` asks the
//! stronger model. `conjure lsp` is the editor's language server, which lands changes.
//!
//! Each command reads its arguments, calls one use case and prints what comes back. The exit
//! code is 0 when a change was conjured (or none was needed), and 2 when conjure could not run.

// spellbook:region cli-modules (from crates/conjure/src/cli/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
mod arguments;
mod console;
mod cost;
mod doctor;
mod format;
mod log;
mod lsp;
mod run;
mod run_cli;
mod trust;
// spellbook:end cli-modules

use arguments::RunArgs;
pub use doctor::{DoctorRow, doctor_rows};
pub use run_cli::{main, main_with};
