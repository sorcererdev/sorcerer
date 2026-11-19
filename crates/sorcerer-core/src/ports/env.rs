//! The process environment as a seam: variables and executables on PATH.

use std::path::PathBuf;

pub trait Env: Send + Sync {
    fn var(&self, key: &str) -> Option<String>;
    fn which(&self, exe: &str) -> Option<PathBuf>;
}
