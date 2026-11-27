//! Everything that differs between Windows, macOS and Linux, in one place.
//!
//! This directory is the only code in the repository allowed to ask which operating system it
//! runs on (`cfg(unix)`, `cfg(windows)`, `std::os::…`); `scripts/platform` refuses those forms
//! everywhere else, tests included. The rest of sorcerer calls these functions and reads the
//! same on every platform.
//!
//! - `paths`: a canonical path without Windows' `\\?\` prefix; a symlink for tests.
//! - `dirs`: the user's cache and config directories.
//! - `compiler`: the C compiler that builds a grammar, and the shared library's extension.
//! - `process`: starting a program so that stopping it also stops everything it started.
//! - `local_connect`: connecting to a port on this machine, refused at once when nothing listens.

// spellbook:region platform-modules (from crates/sorcerer-core/src/adapters/platform/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod compiler;
pub mod dirs;
pub mod local_connect;
pub mod paths;
pub mod process;
// spellbook:end platform-modules
