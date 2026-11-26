//! The command line. `scry [FILES]...` reviews a change and prints the
//! open findings in it, writing a run log as it goes (`scry log` prints
//! the log afterwards, `-v` streams it as the run goes). The other
//! commands look at what scry has stored: `list`, `why`, `dismiss`, `cost`,
//! `log`, and `doctor`, which says whether scry can run here. `lsp` is the
//! editor's language server; `trust` lets it review a project whose config
//! runs code or moves the API key.
//!
//! Each command reads its arguments, calls one use case and prints what
//! comes back. The exit code is 0 when nothing is open in the change, 1
//! when findings are, and 2 when scry could not run.

// spellbook:region cli-modules (from crates/scry/src/cli/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
mod arguments;
mod console;
mod cost;
mod dismiss;
mod doctor;
mod format;
mod list;
mod log;
mod lsp;
mod review;
mod run_cli;
mod trust;
mod why;
// spellbook:end cli-modules

use arguments::RunArgs;
use console::Console;
pub use doctor::{DoctorRow, doctor_rows};
pub use run_cli::{main, main_with};
