//! The command line, over a stub writer and the vendored grammars.

mod support;

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use conjure::adapters::project_context::FilesContext;
use conjure::adapters::wiring::Wired;
use conjure::cli::main_with;
use conjure::ports::writer::{Writer, Written};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::GrammarSource;
use sorcerer_core::ports::ledger::Ledger;
use support::*;

fn args(rest: &[&str]) -> Vec<OsString> {
    std::iter::once("conjure")
        .chain(rest.iter().copied())
        .map(OsString::from)
        .collect()
}

/// A writer that gives a fixed reply and records its errand in the project's ledger, as the
/// live writers do.
struct LedgerWriter {
    reply: String,
    ledger: Arc<JsonlLedger>,
}

impl Writer for LedgerWriter {
    fn write(&self, _: &str, _: &Cancel) -> sorcerer_models::Result<Written> {
        let errand = errand("conjure", "haiku", 0.001);
        self.ledger.append(&errand).unwrap();
        Ok(Written {
            text: self.reply.clone(),
            errand,
        })
    }

    fn model(&self) -> &str {
        "haiku"
    }
}

/// Runs the command in DIR with the stub writer; the exit code, what it printed and what it said.
fn run(dir: &Path, rest: &[&str], reply: &str) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let env = MapEnv::default().with_user_dirs(dir);
    let reply = reply.to_string();
    let wire = move |mut project: sorcerer_core::domain::project::Project,
                     _: &conjure::domain::config::ConjureConfig,
                     _: conjure::domain::record::Attempt| {
        for name in ["python", "rust"] {
            project
                .languages
                .override_grammar(name, GrammarSource::Local(vendored(name)));
        }
        let ledger = Arc::new(JsonlLedger::for_project(&project)?);
        let project = Arc::new(project);
        Ok(Wired {
            view: Arc::new(FilesView::new(project.clone(), test_cache())),
            context: Box::new(FilesContext {
                project: project.clone(),
                view: Arc::new(FilesView::new(project.clone(), test_cache())),
                cache: test_cache(),
                fences: Fences::for_project(&project).unwrap(),
            }),
            writer: Box::new(LedgerWriter {
                reply: reply.clone(),
                ledger: ledger.clone(),
            }),
            ledger,
            clock: Arc::new(FixedClock::at("2026-11-22T12:00:00Z".parse().unwrap())),
            project: (*project).clone(),
        })
    };
    let code = main_with(args(rest), dir, &env, &wire, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn conjure_at_a_line_prints_the_goal_and_the_diff_and_writes_nothing() {
    let d = cart_project();
    let (code, out, err) = run(d.path(), &["src/shop/cart.py:3"], CART_REPLY);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("goal: fill total"), "{out}");
    assert!(out.contains("--- src/shop/cart.py"), "{out}");
    assert!(out.contains("-        pass\n+        return 0"), "{out}");
    assert!(
        out.contains("--- src/shop/cart__test.py (new file)"),
        "{out}"
    );
    assert!(out.contains("parses"), "{out}");
    assert!(out.contains("haiku · $0.001"), "{out}");
    assert!(
        std::fs::read_to_string(d.path().join("src/shop/cart.py"))
            .unwrap()
            .contains("pass")
    );
    assert!(!d.path().join("src/shop/cart__test.py").exists());
    let ledger =
        JsonlLedger::for_project(&sorcerer_core::domain::project::Project::open(d.path()).unwrap())
            .unwrap();
    assert_eq!(ledger.read().unwrap().errands.len(), 1);
    assert!(
        !d.path()
            .join(".sorcerer/state/conjure/changes.jsonl")
            .exists()
    );
}

#[test]
fn a_range_and_an_absolute_path_are_read() {
    let d = cart_project();
    let absolute = std::fs::canonicalize(d.path().join("src/shop/cart.py")).unwrap();
    let target = format!("{}:5-6", absolute.display());
    let (code, out, err) = run(d.path(), &[&target], "<goal>nothing</goal>");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("no change · nothing"), "{out}");
}

#[test]
fn a_cursor_outside_a_unit_exits_2_with_the_sentence() {
    let d = cart_project();
    write(d.path(), "src/shop/notes.py", "# comment\n");
    let (code, out, err) = run(d.path(), &["src/shop/notes.py:1"], CART_REPLY);
    assert_eq!(code, 2);
    assert!(out.is_empty(), "{out}");
    assert!(
        err.contains("put the cursor in a function to conjure"),
        "{err}"
    );
}

#[test]
fn a_target_without_a_line_or_with_a_missing_file_exits_2() {
    let d = cart_project();
    let (code, _, err) = run(d.path(), &["src/shop/cart.py"], CART_REPLY);
    assert_eq!(code, 2);
    assert!(err.contains("FILE:LINE"), "{err}");
    let (code, _, err) = run(d.path(), &["src/shop/missing.py:1"], CART_REPLY);
    assert_eq!(code, 2);
    assert!(err.contains("missing.py"), "{err}");
}

#[test]
fn dropped_paths_and_a_file_that_does_not_parse_are_said() {
    let d = cart_project();
    let reply = "<goal>g</goal>\n<file path=\"../x.py\">\nx\n</file>\n<file path=\"src/shop/cart.py\">\nclass Cart:\n    def total(self:\n        return 0\n</file>\n";
    let (code, out, _) = run(d.path(), &["src/shop/cart.py:3"], reply);
    assert_eq!(code, 0);
    assert!(out.contains("does not parse (line "), "{out}");
    assert!(out.contains("not written: ../x.py"), "{out}");
}

#[test]
fn version_and_help_exit_0() {
    let d = cart_project();
    let (code, out, _) = run(d.path(), &["--version"], "");
    assert_eq!(code, 0);
    assert!(out.starts_with("conjure "), "{out}");
}
