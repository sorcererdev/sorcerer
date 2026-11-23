//! What the server is built from: the live defaults and the seams tests replace.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sorcerer_core::Cancel;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::system_clock::SystemClock;
use crate::adapters::wiring::{LiveWiring, Wiring};
use crate::ports::clock::Clock;

/// How long the server waits after the last save before it reviews: a burst of saves, or an
/// editor's autosave, becomes one pass. A constant, not a setting.
pub const SETTLE: Duration = Duration::from_secs(2);

/// What the server is built from. The live server takes the real environment, the user's
/// grammar cache and trust list, the 2-second settle, the system clock and the live wiring
/// ([`ServerConfig::live`]); tests give a short settle, a fixed clock, vendored grammars and
/// doubles.
pub struct ServerConfig<W: Wiring> {
    /// Opens the project at a root; called afresh at each save, so config edits need no
    /// restart.
    pub open: OpenProject,
    pub cache: GrammarCache,
    /// The user's trusted projects; `None` when there is no place to keep the list.
    pub trust: Option<TrustList>,
    pub settle: Duration,
    pub clock: Arc<dyn Clock>,
    /// Builds a pass's wiring, given its two cancels: the review calls' and everything
    /// else's.
    pub wire: WirePass<W>,
}

/// Opens the project at a root.
pub type OpenProject = Box<dyn Fn(&Path) -> anyhow::Result<Project> + Send + Sync>;

/// Builds one pass's wiring from the project and the pass's review cancel and other cancel.
pub type WirePass<W> = Box<dyn Fn(&Project, Cancel, Cancel) -> anyhow::Result<W> + Send + Sync>;

impl ServerConfig<LiveWiring> {
    /// The real server: `Project::open`, the user's grammar cache and trust list, the
    /// 2-second settle, the system clock and `LiveWiring::open_with`.
    pub fn live(env: Arc<dyn Env>) -> ServerConfig<LiveWiring> {
        ServerConfig {
            open: Box::new(|root| Ok(Project::open(root)?)),
            cache: GrammarCache::user(&*env),
            trust: TrustList::user(&*env),
            settle: SETTLE,
            clock: Arc::new(SystemClock),
            wire: Box::new(move |project, review, rest| {
                LiveWiring::open_with(project, &*env, review, rest)
            }),
        }
    }
}
