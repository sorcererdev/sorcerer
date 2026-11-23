//! What the server is built from: the live defaults and the seams tests replace.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::system_clock::SystemClock;
use crate::ports::clock::Clock;

/// What the server is built from. The live server takes the real environment, the user's
/// grammar cache and trust list and the system clock ([`ServerConfig::live`]); tests give
/// vendored grammars and a clock they move.
pub struct ServerConfig {
    /// Opens the project at a root; called afresh for each command, so config edits need no
    /// restart.
    pub open: OpenProject,
    pub cache: GrammarCache,
    /// The user's trusted projects; `None` when there is no place to keep the list.
    pub trust: Option<TrustList>,
    pub clock: Arc<dyn Clock>,
}

/// Opens the project at a root.
pub type OpenProject = Box<dyn Fn(&Path) -> anyhow::Result<Project> + Send + Sync>;

impl ServerConfig {
    /// The real server: `Project::open`, the user's grammar cache and trust list, the system
    /// clock.
    pub fn live(env: &dyn Env) -> ServerConfig {
        ServerConfig {
            open: Box::new(|root| Ok(Project::open(root)?)),
            cache: GrammarCache::user(env),
            trust: TrustList::user(env),
            clock: Arc::new(SystemClock),
        }
    }
}
