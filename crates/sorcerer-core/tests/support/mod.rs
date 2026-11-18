//! Helpers shared by core's integration tests.
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Runs git in DIR, panicking with its stderr on failure.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A fresh git repository with one empty commit.
pub fn git_project() -> TempDir {
    let d = TempDir::new().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["config", "user.name", "t"]);
    git(d.path(), &["config", "user.email", "t@example.com"]);
    git(d.path(), &["commit", "-q", "--allow-empty", "-m", "root"]);
    d
}

/// Writes TEXT at REL under DIR, creating directories.
pub fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// Commits everything in DIR.
pub fn commit(dir: &Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// The vendored grammar sources for NAME (no network).
pub fn vendored(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/grammars")
        .join(name)
}

/// The shipped languages with each grammar pointed at its vendored sources.
pub fn languages_with_vendored_grammars() -> sorcerer_core::language::Languages {
    let d = TempDir::new().unwrap();
    let mut ls = sorcerer_core::language::Languages::assemble(d.path(), &[]).unwrap();
    for name in ["python", "rust"] {
        ls.override_grammar(
            name,
            sorcerer_core::language::GrammarSource::Local(vendored(name)),
        );
    }
    ls
}

/// A grammar cache shared by every test in this process (built once).
pub fn test_cache() -> sorcerer_core::grammar::GrammarCache {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("grammar-cache");
    sorcerer_core::grammar::GrammarCache::at(dir)
}
