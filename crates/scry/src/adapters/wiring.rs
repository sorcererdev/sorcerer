//! The live wiring of a review: the project's view, the reviewer over a
//! researcher on the chosen provider, Jev, the claim checker (each
//! recording its errands as scry's `review`, `check`, `decide`), the
//! finding store, the ledger, the settings and the system clock.

use std::sync::Arc;

use anyhow::{Context, Result};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::Languages;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::adapters::claude_code::ClaudeCodeResearcher;
use sorcerer_models::adapters::jev::Jev;
use sorcerer_models::adapters::providers::default_provider_for;
use sorcerer_models::adapters::tool_loop::LoopResearcher;
use sorcerer_models::domain::claim::Claim;
use sorcerer_models::ports::decider::Decider;
use sorcerer_models::ports::researcher::Researcher;
use sorcerer_models::usecases::check_claim::{CheckClaimDeps, check_claim};

use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::research_reviewer::ResearchReviewer;
use crate::adapters::scry_toml::ScryToml;
use crate::adapters::system_clock::SystemClock;
use crate::ports::claim_checker::ClaimChecker;
use crate::ports::run_log::Quiet;
use crate::usecases::review_change::{Progress, ReviewDeps};

/// The product scry's errands are recorded under.
pub const PRODUCT: &str = "scry";

/// What a review needs, however it was built: the live wiring or a test's
/// doubles.
pub trait Wiring {
    fn deps(&self) -> ReviewDeps<'_>;
}

pub struct LiveWiring {
    view: Arc<FilesView>,
    reviewer: ResearchReviewer,
    jev: Jev,
    checker: Box<dyn ClaimChecker>,
    store: JsonFindingStore,
    ledger: JsonlLedger,
    settings: ScryToml,
    clock: SystemClock,
    cancel: Cancel,
}

fn quiet(_: &Progress) {}

impl LiveWiring {
    /// The live wiring for PROJECT: Jev from `[decide]` (its key is
    /// required), the default provider (`claude` runs Claude Code as the
    /// researcher; any other provider runs sorcerer's tool loop over it),
    /// grammars from the user's cache.
    pub fn open(project: &Project, env: &dyn Env) -> Result<LiveWiring> {
        let jev = jev_for(project, env)?;
        let view = Arc::new(FilesView::new(
            Arc::new(project.clone()),
            GrammarCache::user(env),
        ));
        let fences = Fences::for_project(project)?;
        let researcher = researcher_for(project, env, view.clone())?;
        let cancel = Cancel::default();
        let checker = claim_checker(
            researcher.clone(),
            Arc::new(jev.clone()),
            fences,
            view.clone(),
            project.languages.clone(),
            cancel.clone(),
        );
        Ok(LiveWiring {
            view,
            reviewer: ResearchReviewer { researcher },
            jev,
            checker,
            store: JsonFindingStore::for_project(project)?,
            ledger: JsonlLedger::for_project(project)?,
            settings: ScryToml::for_project(project),
            clock: SystemClock,
            cancel,
        })
    }
}

impl Wiring for LiveWiring {
    fn deps(&self) -> ReviewDeps<'_> {
        ReviewDeps {
            view: &*self.view,
            reviewer: &self.reviewer,
            decider: &self.jev,
            checker: &*self.checker,
            store: &self.store,
            ledger: &self.ledger,
            settings: &self.settings,
            clock: &self.clock,
            log: &Quiet,
            progress: &quiet,
            cancel: &self.cancel,
        }
    }
}

/// The researcher on PROJECT's default provider: `claude` runs Claude Code;
/// any other provider runs sorcerer's tool loop over it, reading through VIEW
/// within the project's fences and recording into the project's ledger.
pub fn researcher_for(
    project: &Project,
    env: &dyn Env,
    view: Arc<dyn CodebaseView>,
) -> Result<Arc<dyn Researcher>> {
    let (provider, choice) = default_provider_for(project, env)?;
    let cfg = project
        .config
        .model
        .providers
        .get(&choice.name)
        .cloned()
        .unwrap_or_default();
    let kind = cfg.kind.clone().unwrap_or_else(|| choice.name.clone());
    Ok(if kind == "claude" {
        Arc::new(ClaudeCodeResearcher::new(project, &cfg)?)
    } else {
        Arc::new(LoopResearcher::new(
            provider,
            view,
            Fences::for_project(project)?,
            Arc::new(JsonlLedger::for_project(project)?),
        ))
    })
}

/// Jev from `[decide]`, recording into the project's ledger as scry's
/// `decide` errands. No key is an error naming the variable to set.
pub fn jev_for(project: &Project, env: &dyn Env) -> Result<Jev> {
    let jev = Jev::from_env(
        &project.config.decide,
        env,
        Some(Arc::new(JsonlLedger::for_project(project)?)),
    )
    .context("scry does not run without Jev")?;
    Ok(jev.for_product(PRODUCT))
}

/// The claim checker as a review calls it: `check_claim` with the shipped
/// defaults, its errands recorded as scry's `check`, Jev guarding, a unit
/// named the way the project's LANGUAGES name the claim's files' units.
pub fn claim_checker(
    researcher: Arc<dyn Researcher>,
    decider: Arc<dyn Decider>,
    fences: Fences,
    view: Arc<dyn CodebaseView>,
    languages: Languages,
    cancel: Cancel,
) -> Box<dyn ClaimChecker> {
    Box::new(move |claim: &Claim| {
        let mut deps =
            CheckClaimDeps::defaults(&*researcher, Some(&*decider), &fences, &*view, PRODUCT);
        deps.languages = Some(&languages);
        check_claim(claim, &deps, &cancel)
    })
}
