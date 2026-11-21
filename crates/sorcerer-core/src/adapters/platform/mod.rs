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

pub mod compiler;
pub mod dirs;
pub mod paths;
pub mod process;
