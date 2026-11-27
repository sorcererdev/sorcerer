//! What the server is built from: the live defaults and the seams tests replace.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::view::CodebaseView;

use crate::adapters::spellbook_checker::SpellbookChecker;
use crate::adapters::system_clock::SystemClock;
use crate::adapters::wiring::writer_for;
use crate::domain::config::ConjureConfig;
use crate::domain::record::Attempt;
use crate::ports::clock::Clock;
use crate::ports::rule_checker::RuleChecker;
use crate::ports::writer::Writer;

/// What the server is built from. The live server takes the real environment, the user's
/// grammar cache and trust list, the system clock and the live writers ([`ServerConfig::live`]);
/// tests give vendored grammars, a fixed clock and a scripted writer.
pub struct ServerConfig {
    /// Opens the project at a root; called afresh for each change, so config edits need no
    /// restart.
    pub open: OpenProject,
    pub cache: GrammarCache,
    /// The user's trusted projects; `None` when there is no place to keep the list.
    pub trust: Option<TrustList>,
    pub clock: Arc<dyn Clock>,
    /// Builds the writer of an attempt, reading the project through the editor's view.
    pub wire: WireWriter,
    /// Builds the checker of the project's rules (the installed `spellbook`, live).
    pub checker: CheckerFor,
}

/// Builds the rule checker for a project.
pub type CheckerFor = Box<dyn Fn(&Project) -> Box<dyn RuleChecker> + Send + Sync>;

/// Opens the project at a root.
pub type OpenProject = Box<dyn Fn(&Path) -> anyhow::Result<Project> + Send + Sync>;

/// Builds the writer for a project, its settings and an attempt; the view is the editor's, so a
/// writer that reads the project (Try harder) reads the unsaved text.
pub type WireWriter = Box<
    dyn Fn(
            &Project,
            &ConjureConfig,
            Attempt,
            Arc<dyn CodebaseView>,
        ) -> anyhow::Result<Box<dyn Writer>>
        + Send
        + Sync,
>;

impl ServerConfig {
    /// The real server: `Project::open`, the user's grammar cache and trust list, the system
    /// clock, `writer_for` and the installed `spellbook`.
    pub fn live(env: Arc<dyn Env>) -> ServerConfig {
        let checker_env = env.clone();
        ServerConfig {
            open: Box::new(|root| Ok(Project::open(root)?)),
            cache: GrammarCache::user(&*env),
            trust: TrustList::user(&*env),
            clock: Arc::new(SystemClock),
            wire: Box::new(move |project, config, attempt, view| {
                writer_for(project, &*env, config, attempt, view)
            }),
            checker: Box::new(move |project| {
                Box::new(SpellbookChecker::for_project(project, &*checker_env))
            }),
        }
    }
}
