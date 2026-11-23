//! The fake `claude` program the tests run, and the files it writes into.

use std::path::PathBuf;

/// The fake `claude` program (`tests/support/fake_claude.rs`).
pub fn fake_claude() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-claude"))
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
