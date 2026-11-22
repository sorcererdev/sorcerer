//! The live wiring: the project's files, the writer on the project's provider, the ledger and
//! the system clock. Each model call is recorded in the ledger as one of conjure's errands:
//! `conjure` for the first attempt, `try-harder` for the second.

use std::sync::Arc;

use anyhow::{Result, bail};
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::adapters::providers::default_provider_for;
use sorcerer_models::adapters::researchers::researcher_for;

use crate::adapters::project_context::FilesContext;
use crate::adapters::provider_writer::ProviderWriter;
use crate::adapters::research_writer::ResearchWriter;
use crate::adapters::system_clock::SystemClock;
use crate::domain::config::ConjureConfig;
use crate::domain::record::Attempt;
use crate::ports::clock::Clock;
use crate::ports::project_context::ProjectContext;
use crate::ports::writer::Writer;

/// Everything a change needs besides the request, however it was built: the live wiring, or a
/// test's doubles.
pub struct Wired {
    pub project: Project,
    pub view: Arc<dyn CodebaseView>,
    pub context: Box<dyn ProjectContext>,
    pub writer: Box<dyn Writer>,
    pub ledger: Arc<dyn Ledger>,
    pub clock: Arc<dyn Clock>,
}

/// How the command line builds what it needs for the project and the attempt; the tests pass
/// their own.
pub type Wire<'a> = &'a dyn Fn(Project, &ConjureConfig, Attempt) -> Result<Wired>;

/// The live wiring: grammars from the user's cache, the project's default provider, the system
/// clock.
pub fn live(
    project: Project,
    env: &dyn Env,
    config: &ConjureConfig,
    attempt: Attempt,
) -> Result<Wired> {
    let cache = GrammarCache::user(env);
    let view: Arc<dyn CodebaseView> =
        Arc::new(FilesView::new(Arc::new(project.clone()), cache.clone()));
    let writer = writer_for(&project, env, config, attempt, view.clone())?;
    let ledger: Arc<dyn Ledger> = Arc::new(JsonlLedger::for_project(&project)?);
    let context = FilesContext {
        project: Arc::new(project.clone()),
        view: view.clone(),
        cache,
        fences: Fences::for_project(&project)?,
    };
    Ok(Wired {
        project,
        view,
        context: Box::new(context),
        writer,
        ledger,
        clock: Arc::new(SystemClock),
    })
}

/// The writer for an attempt. The first is a plain call to `[conjure] writer` on the project's
/// default provider; Try harder is `[conjure] try_harder` as a researcher that may read, grep
/// and glob the project first, and is refused when `try_harder` is empty.
pub fn writer_for(
    project: &Project,
    env: &dyn Env,
    config: &ConjureConfig,
    attempt: Attempt,
    view: Arc<dyn CodebaseView>,
) -> Result<Box<dyn Writer>> {
    match attempt {
        Attempt::First => {
            let (provider, _) = default_provider_for(project, env)?;
            Ok(Box::new(ProviderWriter {
                provider,
                model: config.writer.clone(),
                ledger: Arc::new(JsonlLedger::for_project(project)?),
                clock: Arc::new(SystemClock),
            }))
        }
        Attempt::TryHarder if !config.offers_try_harder() => {
            bail!("Try harder is off: [conjure] try_harder is empty")
        }
        Attempt::TryHarder => Ok(Box::new(ResearchWriter {
            researcher: researcher_for(project, env, view)?,
            model: config.try_harder.clone(),
        })),
    }
}
