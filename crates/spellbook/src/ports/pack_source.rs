//! Where a pack comes from, and the project folder it is copied into.

use crate::domain::pack::{PackAddress, PackStop};

/// A pack fetched: the commit it was taken at, its root `pack.toml` (when it has one) and every
/// plain file under its `.sorcerer/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedPack {
    pub rev: String,
    pub manifest: Option<String>,
    pub files: Vec<PackFile>,
}

/// One file of a pack's `.sorcerer/`: its path there, with `/`, and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

pub trait PackSource {
    /// Fetches the pack at its revision (the default branch when none is given).
    fn fetch(&self, address: &PackAddress) -> Result<FetchedPack, PackStop>;
}

/// The project's `.sorcerer/` folder, paths relative to it with `/`.
pub trait SorcererDir {
    /// Whether a file or folder is there.
    fn holds(&self, path: &str) -> bool;
    /// Writes a file, its folders created, over any file there.
    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), PackStop>;
}
