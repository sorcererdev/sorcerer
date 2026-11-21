//! The user's list of trusted projects: `trusted.toml` in the user's sorcerer config directory.
//!
//! A project whose own files set something that runs code or moves the API key
//! (`domain/trust.rs`) is acted on by an editor's server only once its root is on this list.
//! The file reads `projects = ["/abs/root", …]`, sorted, and is written whole to a temporary
//! file beside it and renamed over it, so a reader never sees half a list.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ports::env::Env;
use crate::{Error, Result};

/// The trusted-projects file. Making one reads nothing; each question reads the file afresh,
/// so a `scry trust` in a terminal is seen by a server already running.
#[derive(Debug, Clone)]
pub struct TrustList {
    file: PathBuf,
}

/// What `trusted.toml` holds.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedFile {
    #[serde(default)]
    projects: BTreeSet<String>,
}

impl TrustList {
    /// The user's list: `$XDG_CONFIG_HOME/sorcerer/trusted.toml`, else
    /// `~/.config/sorcerer/trusted.toml`. `None` when neither variable is set, so there is no
    /// place to keep it.
    pub fn user(env: &dyn Env) -> Option<TrustList> {
        let base = env
            .var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env.var("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(TrustList::at(&base.join("sorcerer")))
    }

    /// A list kept as `trusted.toml` in any directory, for tests.
    pub fn at(dir: &Path) -> TrustList {
        TrustList {
            file: dir.join("trusted.toml"),
        }
    }

    /// Where the list is kept.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Whether a project root is on the list. A list that cannot be read trusts nothing.
    pub fn is_trusted(&self, root: &Path) -> bool {
        self.read()
            .is_ok_and(|list| list.projects.contains(&root_key(root)))
    }

    /// Puts a project root on the list; trusting it twice is the same as once.
    pub fn trust(&self, root: &Path) -> Result<()> {
        let mut list = self.read()?;
        list.projects.insert(root_key(root));
        self.write(&list)
    }

    /// Takes a project root off the list. Answers whether it was there.
    pub fn revoke(&self, root: &Path) -> Result<bool> {
        let mut list = self.read()?;
        let was_there = list.projects.remove(&root_key(root));
        if was_there {
            self.write(&list)?;
        }
        Ok(was_there)
    }

    /// The list as it is on disk; a missing file is an empty list.
    fn read(&self) -> Result<TrustedFile> {
        match std::fs::read_to_string(&self.file) {
            Ok(text) => toml::from_str(&text).map_err(|e| Error::Config {
                file: self.file.clone(),
                why: e.message().to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TrustedFile::default()),
            Err(e) => Err(Error::io(&self.file, e)),
        }
    }

    /// Writes the whole list beside the file, then renames it over the file.
    fn write(&self, list: &TrustedFile) -> Result<()> {
        let dir = self.file.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let text = toml::to_string(list).map_err(|e| Error::Config {
            file: self.file.clone(),
            why: e.to_string(),
        })?;
        let beside = self.file.with_extension("toml.tmp");
        std::fs::write(&beside, text).map_err(|e| Error::io(&beside, e))?;
        std::fs::rename(&beside, &self.file).map_err(|e| Error::io(&self.file, e))
    }
}

/// How a root is written in the list: its path as text.
fn root_key(root: &Path) -> String {
    root.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::env::MapEnv;
    use crate::domain::project::Project;

    #[test]
    fn the_list_round_trips_and_stays_sorted() {
        let dir = tempfile::TempDir::new().unwrap();
        let list = TrustList::at(dir.path());
        list.trust(Path::new("/b")).unwrap();
        list.trust(Path::new("/a")).unwrap();
        let text = std::fs::read_to_string(dir.path().join("trusted.toml")).unwrap();
        assert_eq!(text.trim(), "projects = [\"/a\", \"/b\"]");
        assert!(list.is_trusted(Path::new("/a")));
        assert!(list.revoke(Path::new("/a")).unwrap());
        assert!(!list.revoke(Path::new("/a")).unwrap());
        assert!(!list.is_trusted(Path::new("/a")));
        assert!(list.is_trusted(Path::new("/b")));
    }

    #[test]
    fn a_missing_list_trusts_nothing_and_a_broken_one_refuses_to_be_written() {
        let dir = tempfile::TempDir::new().unwrap();
        let list = TrustList::at(dir.path());
        assert!(!list.is_trusted(Path::new("/a")));
        std::fs::write(dir.path().join("trusted.toml"), "projects = [").unwrap();
        assert!(!list.is_trusted(Path::new("/a")));
        assert!(list.trust(Path::new("/a")).is_err());
        assert!(list.revoke(Path::new("/a")).is_err());
    }

    #[test]
    fn a_symlink_and_its_target_are_one_project() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir_all(target.join(".sorcerer")).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let list = TrustList::at(&dir.path().join("config"));
        list.trust(&Project::open(&link).unwrap().root).unwrap();
        assert!(list.is_trusted(&Project::open(&target).unwrap().root));
    }

    #[test]
    fn user_follows_xdg_then_home() {
        let xdg = MapEnv::default()
            .var_set("XDG_CONFIG_HOME", "/x")
            .var_set("HOME", "/h");
        assert_eq!(
            TrustList::user(&xdg).unwrap().file(),
            Path::new("/x/sorcerer/trusted.toml")
        );
        let home = MapEnv::default().var_set("HOME", "/h");
        assert_eq!(
            TrustList::user(&home).unwrap().file(),
            Path::new("/h/.config/sorcerer/trusted.toml")
        );
        assert!(TrustList::user(&MapEnv::default()).is_none());
    }
}
