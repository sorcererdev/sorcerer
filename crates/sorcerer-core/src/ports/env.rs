//! The process environment as a seam: variables and executables on PATH.

use std::path::PathBuf;

/// What sorcerer asks of the process environment, so tests can give a fixed one.
pub trait Env: Send + Sync {
    /// An environment variable's value; `None` when it is unset or empty.
    fn var(&self, key: &str) -> Option<String>;
    /// Where an executable is on PATH, such as `cc` or `claude`; `None` when it is not there.
    /// On Windows the extensions in `PATHEXT` are tried too, so `claude` finds `claude.cmd`
    /// (npm's shim) or `claude.exe`.
    fn which(&self, exe: &str) -> Option<PathBuf>;
}
