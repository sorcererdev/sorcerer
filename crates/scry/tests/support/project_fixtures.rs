//! A project on the vendored grammars, and a ledger row to put in it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::language::GrammarSource;
use sorcerer_core::domain::project::Project;

/// A grammar cache shared by every test in this process (built once).
pub fn test_cache() -> GrammarCache {
    GrammarCache::at(Path::new(env!("CARGO_TARGET_TMPDIR")).join("grammar-cache"))
}

/// The project at DIR with its grammars pointed at the vendored sources, and
/// a view of it.
pub fn project_with_vendored_grammars(dir: &Path) -> (Arc<Project>, FilesView) {
    let mut p = Project::open(dir).unwrap();
    super::point_at_test_grammars(&mut p.languages);
    let p = Arc::new(p);
    let v = FilesView::new(p.clone(), test_cache());
    (p, v)
}

/// An errand as a paid call would append it.
pub fn errand(verb: &str, model: &str, cost: f64) -> Errand {
    Errand {
        at: Utc::now(),
        product: "scry".into(),
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
