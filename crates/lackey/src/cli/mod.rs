//! The command line. `lackey FILE:LINE "instruction"` runs one task from the terminal (its
//! changes written to the disk); the editor's server is the product. `lackey mcp` and
//! `lackey hook` are the helpers a task's agent runs; `cost`, `log`, `doctor` and `trust`
//! are as in the other products.

// spellbook:region cli-modules (from crates/lackey/src/cli/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
mod arguments;
mod console;
mod cost;
mod doctor;
mod format;
mod hook;
mod log;
mod lsp;
mod mcp;
mod run;
mod run_cli;
mod trust;
// spellbook:end cli-modules

use arguments::RunArgs;
pub use doctor::{DoctorRow, Finding, doctor_rows};
pub use run_cli::{Wiring, main, main_with};
