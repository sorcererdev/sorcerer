//! Helpers shared by the models crate's integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The fake `claude` script.
pub fn fake_claude() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake-claude")
}

/// Files the fake writes into, and the variables pointing it at them.
pub struct FakeClaude {
    pub dir: tempfile::TempDir,
}

impl FakeClaude {
    pub fn new() -> FakeClaude {
        FakeClaude {
            dir: tempfile::TempDir::new().unwrap(),
        }
    }

    pub fn envs(&self, extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let p = |f: &str| self.dir.path().join(f).to_string_lossy().into_owned();
        let mut v = vec![
            ("FAKE_CLAUDE_ARGS".to_string(), p("args")),
            ("FAKE_CLAUDE_STDIN".to_string(), p("stdin")),
            ("FAKE_CLAUDE_ENV".to_string(), p("env")),
        ];
        v.extend(
            extra
                .iter()
                .map(|(k, val)| (k.to_string(), val.to_string())),
        );
        v
    }

    pub fn args(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("args"))
            .unwrap()
            .lines()
            .map(String::from)
            .collect()
    }

    pub fn stdin(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("stdin")).unwrap()
    }

    pub fn env(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("env")).unwrap()
    }
}

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
