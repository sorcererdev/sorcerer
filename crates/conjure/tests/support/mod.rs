//! Helpers shared by conjure's integration tests: git fixtures, a project on the vendored
//! grammars, and doubles for the writer and the clock.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset, Utc};
use conjure::ports::clock::Clock;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::language::GrammarSource;
use sorcerer_core::domain::project::Project;
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

/// Core's vendored grammar sources for NAME (no network).
pub fn vendored(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sorcerer-core/tests/grammars")
        .join(name)
}

/// A grammar cache shared by every test in this process (built once).
pub fn test_cache() -> GrammarCache {
    GrammarCache::at(Path::new(env!("CARGO_TARGET_TMPDIR")).join("grammar-cache"))
}

/// The project at DIR with its grammars pointed at the vendored sources, and a view of it.
pub fn project_with_vendored_grammars(dir: &Path) -> (Arc<Project>, FilesView) {
    let mut p = Project::open(dir).unwrap();
    for name in ["python", "rust"] {
        p.languages
            .override_grammar(name, GrammarSource::Local(vendored(name)));
    }
    let p = Arc::new(p);
    let v = FilesView::new(p.clone(), test_cache());
    (p, v)
}

/// A clock that says what it is told, in a zone it is told (UTC unless set).
pub struct FixedClock {
    at: Mutex<DateTime<Utc>>,
    zone: FixedOffset,
}

impl FixedClock {
    pub fn at(t: DateTime<Utc>) -> FixedClock {
        FixedClock {
            at: Mutex::new(t),
            zone: FixedOffset::east_opt(0).unwrap(),
        }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.at.lock().unwrap()
    }

    fn offset(&self) -> FixedOffset {
        self.zone
    }
}

/// An errand as a paid call would append it.
pub fn errand(verb: &str, model: &str, cost: f64) -> Errand {
    Errand {
        at: Utc::now(),
        product: "conjure".into(),
        verb: verb.into(),
        model: model.into(),
        provider: "stub".into(),
        cost_usd: Some(cost),
        input_tokens: 0,
        output_tokens: 0,
        seconds: 0.0,
        ok: true,
        note: None,
    }
}
