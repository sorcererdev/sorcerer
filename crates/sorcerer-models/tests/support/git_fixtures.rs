//! Throwaway git repositories to test in.

use std::path::Path;

/// A fresh git repository with one empty commit and, when given, a config.
pub fn git_project(config: Option<&str>) -> tempfile::TempDir {
    let d = tempfile::TempDir::new().unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(d.path())
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        )
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "t"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["commit", "-q", "--allow-empty", "-m", "root"]);
    if let Some(c) = config {
        write(d.path(), ".sorcerer/config.toml", c);
    }
    d
}

/// Writes TEXT at REL under DIR, creating directories.
pub fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
