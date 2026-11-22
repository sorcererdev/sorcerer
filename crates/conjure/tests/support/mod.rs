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

impl FixedClock {
    /// The same clock in ZONE (the user's local offset).
    pub fn in_zone(mut self, zone: FixedOffset) -> FixedClock {
        self.zone = zone;
        self
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

// The change use case's rig.

use conjure::adapters::project_context::FilesContext;
use conjure::domain::config::{ConjureConfig, conjure_config};
use conjure::ports::writer::{Writer, Written};
use conjure::usecases::conjure_change::ChangeDeps;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;

/// A writer that answers one fixed reply and keeps every situation it was shown.
pub struct StubWriter {
    pub reply: String,
    pub seen: Mutex<Vec<String>>,
}

impl StubWriter {
    pub fn replying(reply: &str) -> StubWriter {
        StubWriter {
            reply: reply.into(),
            seen: Mutex::new(Vec::new()),
        }
    }

    pub fn situations(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

impl Writer for StubWriter {
    fn write(&self, situation: &str, _: &Cancel) -> sorcerer_models::Result<Written> {
        self.seen.lock().unwrap().push(situation.into());
        Ok(Written {
            text: self.reply.clone(),
            errand: errand("conjure", "haiku", 0.001),
        })
    }

    fn model(&self) -> &str {
        "haiku"
    }
}

/// Everything `conjure_change` is given, over a project on the vendored grammars, a stub
/// writer and a clock fixed at 2026-11-22 12:00 UTC.
pub struct Rig {
    pub project: Arc<Project>,
    pub view: FilesView,
    pub context: FilesContext,
    pub fences: Fences,
    pub config: ConjureConfig,
    pub writer: StubWriter,
    pub ledger: JsonlLedger,
    pub clock: FixedClock,
    pub cancel: Cancel,
}

impl Rig {
    pub fn new(dir: &Path, reply: &str) -> Rig {
        let (project, view) = project_with_vendored_grammars(dir);
        let context = FilesContext {
            project: project.clone(),
            view: Arc::new(FilesView::new(project.clone(), test_cache())),
            cache: test_cache(),
            fences: Fences::for_project(&project).unwrap(),
        };
        Rig {
            fences: Fences::for_project(&project).unwrap(),
            config: conjure_config(&project).unwrap(),
            ledger: JsonlLedger::for_project(&project).unwrap(),
            project,
            view,
            context,
            writer: StubWriter::replying(reply),
            clock: FixedClock::at("2026-11-22T12:00:00Z".parse().unwrap()),
            cancel: Cancel::default(),
        }
    }

    pub fn deps(&self) -> ChangeDeps<'_> {
        ChangeDeps {
            project: &self.project,
            view: &self.view,
            context: &self.context,
            fences: &self.fences,
            config: &self.config,
            writer: &self.writer,
            ledger: &self.ledger,
            clock: &self.clock,
            cancel: &self.cancel,
        }
    }
}

/// A committed project holding a stubbed cart, as `conjure` meets it.
pub fn cart_project() -> TempDir {
    let d = git_project();
    write(
        d.path(),
        "src/shop/cart.py",
        "class Cart:\n    def total(self):\n        pass\n\n    def empty(self):\n        return True\n",
    );
    write(d.path(), "README.md", "# shop\n");
    commit(d.path(), "cart");
    d
}

/// A reply that writes the cart's `total` and a new test file beside it.
pub const CART_REPLY: &str = "<goal>fill total</goal>\n<file path=\"src/shop/cart.py\">\nclass Cart:\n    def total(self):\n        return 0\n\n    def empty(self):\n        return True\n</file>\n<file path=\"src/shop/cart__test.py\">\nfrom shop.cart import Cart\n\n\ndef test_total():\n    assert Cart().total() == 0\n</file>\n";
