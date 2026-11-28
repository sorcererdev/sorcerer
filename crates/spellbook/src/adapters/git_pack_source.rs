//! Packs fetched with the `git` command line: a clone into a temporary folder (shallow when no
//! revision is asked for), the revision checked out, and the `.sorcerer/` files read. Only `git`
//! is run, so it works the same on Windows, macOS and Linux.

use std::path::Path;
use std::process::Command;

use sorcerer_core::domain::paths::forward_slashes;

use crate::domain::pack::{PackAddress, PackStop};
use crate::ports::pack_source::{FetchedPack, PackFile, PackSource};

/// Fetches packs by running `git`.
pub struct GitPackSource;

impl PackSource for GitPackSource {
    /// (1) clones the repository into a temporary folder (`--depth 1` when no revision is asked
    /// for); (2) checks out the revision; (3) reads the commit taken; (4) reads the root
    /// `pack.toml` and every file under `.sorcerer/` but its `state/`. The folder is removed when
    /// this returns.
    fn fetch(&self, address: &PackAddress) -> Result<FetchedPack, PackStop> {
        let checkout = tempfile::tempdir().map_err(|e| PackStop::Disk(e.to_string()))?;
        let dir = checkout.path();
        clone(address, dir)?;
        if let Some(rev) = &address.rev {
            git(dir, &["checkout", "--quiet", "--detach", rev])?;
        }
        let rev = git(dir, &["rev-parse", "HEAD"])?.trim().to_string();
        let sorcerer = dir.join(".sorcerer");
        if !sorcerer.is_dir() {
            return Err(PackStop::NotAPack);
        }
        Ok(FetchedPack {
            rev,
            manifest: std::fs::read_to_string(dir.join("pack.toml")).ok(),
            files: files_under(&sorcerer)?,
        })
    }
}

fn clone(address: &PackAddress, into: &Path) -> Result<(), PackStop> {
    // The pack's files arrive as committed: a Windows `core.autocrlf` would turn them CRLF.
    let mut args = vec!["clone", "--quiet", "--config", "core.autocrlf=false"];
    if address.rev.is_none() {
        args.extend(["--depth", "1"]);
    }
    let into = into.to_string_lossy();
    args.extend(["--", address.repo.as_str(), into.as_ref()]);
    git(Path::new("."), &args).map(|_| ())
}

/// Runs `git` in a folder and returns what it printed; a failure is its first line of complaint.
/// It never waits for a password: a repository that asks for one fails.
fn git(dir: &Path, args: &[&str]) -> Result<String, PackStop> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| PackStop::Fetch(format!("git could not be run ({e}); is it installed?")))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let complaint = String::from_utf8_lossy(&output.stderr);
    let first = complaint
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("warning:"))
        .unwrap_or("git failed");
    Err(PackStop::Fetch(format!("git {}: {first}", args[0])))
}

/// Every plain file under the pack's `.sorcerer/`, by its path there with `/`, sorted; its
/// `state/` (what products remember, never part of a pack) is left out, and a link stops it.
fn files_under(sorcerer: &Path) -> Result<Vec<PackFile>, PackStop> {
    let mut found = Vec::new();
    collect(sorcerer, sorcerer, &mut found)?;
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

fn collect(top: &Path, dir: &Path, found: &mut Vec<PackFile>) -> Result<(), PackStop> {
    let disk = |e: std::io::Error| PackStop::Disk(format!("{}: {e}", dir.display()));
    for entry in std::fs::read_dir(dir).map_err(disk)? {
        let path = entry.map_err(disk)?.path();
        let relative = path
            .strip_prefix(top)
            .map(forward_slashes)
            .unwrap_or_default();
        let kind = std::fs::symlink_metadata(&path).map_err(disk)?.file_type();
        if kind.is_symlink() {
            return Err(PackStop::Link(relative));
        }
        if kind.is_dir() {
            if relative != "state" {
                collect(top, &path, found)?;
            }
            continue;
        }
        let bytes = std::fs::read(&path).map_err(disk)?;
        found.push(PackFile {
            path: relative,
            bytes,
        });
    }
    Ok(())
}
