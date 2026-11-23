//! The change use case's rig: a stub writer and everything `conjure_change` is given.

use std::path::Path;
use std::sync::{Arc, Mutex};

use super::{
    FixedClock, commit, errand, git_project, project_with_vendored_grammars, test_cache, write,
};
use conjure::adapters::project_context::FilesContext;
use conjure::domain::config::{ConjureConfig, conjure_config};
use conjure::domain::edits::NameCase;
use conjure::ports::writer::{Writer, Written};
use conjure::usecases::conjure_change::ChangeDeps;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use tempfile::TempDir;

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
    pub name_case: NameCase,
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
            name_case: if sorcerer_core::adapters::platform::paths::names_ignore_case(dir) {
                NameCase::Ignored
            } else {
                NameCase::Matters
            },
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
            name_case: self.name_case,
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
