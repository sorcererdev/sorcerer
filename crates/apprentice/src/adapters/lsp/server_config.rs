//! What the server is built from: the live defaults and the seams tests replace.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::adapters::jev::Jev;
use sorcerer_models::adapters::researchers::researcher_for;
use sorcerer_models::ports::decider::Decider;
use sorcerer_models::ports::researcher::Researcher;

use crate::adapters::system_clock::SystemClock;
use crate::adapters::wiring::{WireAgent, live_agents};
use crate::domain::config::PRODUCT;
use crate::ports::clock::Clock;

/// How long after the last keystroke a file is looked at (the idle clock).
pub const IDLE: Duration = Duration::from_millis(1500);

/// What the server is built from. The live server takes the real environment, the user's
/// grammar cache and trust list, the system clock, `claude`, Jev and the project's researcher
/// ([`ServerConfig::live`]); tests give doubles.
pub struct ServerConfig {
    /// Opens the project at a root; called afresh for each look, so config edits need no
    /// restart.
    pub open: OpenProject,
    pub cache: GrammarCache,
    pub trust: Option<TrustList>,
    pub clock: Arc<dyn Clock>,
    pub agent: WireAgent,
    pub decider: WireDecider,
    pub researcher: WireResearcher,
    /// How long after the last keystroke triage runs.
    pub idle: Duration,
}

pub type OpenProject = Box<dyn Fn(&Path) -> anyhow::Result<Project> + Send + Sync>;
/// Jev for a project, recording its calls under apprentice; none when its key is not set.
pub type WireDecider = Box<dyn Fn(&Project) -> Option<Arc<dyn Decider>> + Send + Sync>;
/// The researcher on the project's provider, reading through the view; none without one.
pub type WireResearcher =
    Box<dyn Fn(&Project, Arc<dyn CodebaseView>) -> Option<Arc<dyn Researcher>> + Send + Sync>;

impl ServerConfig {
    /// The real server.
    pub fn live(env: Arc<dyn Env>) -> ServerConfig {
        let for_jev = env.clone();
        ServerConfig {
            open: Box::new(|root| Ok(Project::open(root)?)),
            cache: GrammarCache::user(&*env),
            trust: TrustList::user(&*env),
            clock: Arc::new(SystemClock),
            agent: live_agents(),
            decider: Box::new(move |project| {
                let ledger: Arc<dyn Ledger> = Arc::new(JsonlLedger::for_project(project).ok()?);
                let jev = Jev::from_env(&project.config.decide, &*for_jev, Some(ledger)).ok()?;
                Some(Arc::new(jev.for_product(PRODUCT)))
            }),
            researcher: Box::new(move |project, view| researcher_for(project, &*env, view).ok()),
            idle: IDLE,
        }
    }

    /// The project's files as on disk, for a claim check.
    pub fn disk_view(&self, project: &Project) -> Arc<dyn CodebaseView> {
        Arc::new(FilesView::new(
            Arc::new(project.clone()),
            self.cache.clone(),
        ))
    }
}
